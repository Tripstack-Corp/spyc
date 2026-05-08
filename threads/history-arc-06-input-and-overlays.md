# history-arc-06-input-and-overlays — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-06-input-and-overlays
Created: 2026-05-08T00:36:51.422338+00:00

---
Entry: Claude Code (caleb) 2026-05-08T00:36:51.422338+00:00
Role: scribe
Type: Note
Title: Framing: arc 06 — picker overlays and dispatch correctness across a 2+2 calendar split, cadence option A

Spec: scribe

tags: #history #arc-06

Arc title: `input-and-overlays`. Date span: 2026-05-02 (PR #8) to 2026-05-06 (PR #32). Member PRs:

- 62fc129 (PR #8 feat/harpoon, 2026-05-02) — "harpoon: per-project pinned working set + =h filter (v1.39.0)" (commit 62fc129, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02) — "quick select: ^a u labeled-overlay picker for pane output (v1.40.0)" (commit 9043547, 2026-05-02).
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06) — "fix: input dispatch hardening + --key-trace diagnostic switch (v1.41.12)" (commit bfc4a18, 2026-05-06).
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06) — "fix: chord prefixes beat user keybindings on the second key (v1.41.19)" (commit a7867fb, 2026-05-06).

**Cadence choice: option A (per-PR), with the framing naming the two phases the diff dates make obvious.** Six head entries: framing → 4 per-PR entries → closure. Arc 05 at 8 PRs adopted option A' (per-PR plus phase-grouping doing closure-summarization work that scaled awkwardly at that count); arc 06 at 4 PRs sits below the threshold A' was designed for. Plain A reads cleanly; the 2+2 wall-clock split is small enough that calling out the phases here is a reading aid rather than load-bearing structural work the closure can't carry.

**Phase grouping** (a reading aid for the 2+2 calendar split):

- **Phase α — picker overlays** (PRs #8, #10; both 2026-05-02). Two new picker-shaped overlays land within hours of each other. PR #8 ships harpoon — a per-project pinned working set with an `H1`..`H9` direct-slot-jump chord, an `Hh` modal menu for reorder/delete/jump, an `Ha`/`Hx` append/remove pair, and an `=h` listing filter that surfaces ancestor directories of harpooned paths. PR #10 ships quick select — a `^a u` labeled-overlay scanner over the visible pane output with alphabetic 1- or 2-letter labels (URLs, paths, git SHAs, IPv4, user-defined regex), case-as-intent (lowercase yank, uppercase "open"). Both ship as standalone overlays carrying their own picker structures (`HarpoonMenu`, `QuickSelect`); neither extends `PagerView`.
- **Phase β — dispatch correctness** (PRs #25, #32; both 2026-05-06). Four days later, two fixes to how key events reach handlers. PR #25 ships a "couldn't reproduce, two plausible failure modes addressed" defensive bundle (post-chord bounce-suppression on `^a-j`/`^a-k` plus a stranded-paste flash) bundled with a `--key-trace` / `SPYC_KEY_TRACE` diagnostic switch. PR #32 fixes a single rule: when a chord prefix is pending, the next key resolves the chord instead of being preempted by user keybindings.

A connection between the phases is detectable from the diffs but not asserted in the commits: phase α introduces the new `H` chord prefix family in PR #8, and PR #32's CHANGELOG names `H1`..`H9` explicitly among the broken-chord cases the fix addresses. Whether the connection is sequence-grain (phase α's new chord family surfaced phase β's dispatch question) or coincidental (the dispatch bug was always latent and would have surfaced with any user keybinding hitting an existing chord's second key) is for the per-PR entries to read against the diffs and tests, not for the framing to assert.

**Diagnosis (pattern register from the 10-pattern menu):** capability-accretion (precedent in arcs 04 and 05) followed by corrective hardening. Phase α is the same shape arcs 04 and 05 register: surfaces grow, capabilities accrete, the user gets new ways to summon things by keystroke. Phase β is the corrective follow-on as the surface widens enough that dispatch correctness becomes its own concern — the chord-prefix tree now has more branches, the focus-switch chord is ridden harder, and the picker-overlays have keys of their own that interact with the resolver's pending-state machinery. The interim themes entry (= 01KR2DYTPNCY5J5HPB99GT0J5M) named pattern 8 (reference-inventory) and pattern 10 (hub-and-pivot) as candidate diagnoses. Reference-inventory reads as a forced fit — none of the four PRs pauses to enumerate "what input means" — and hub-and-pivot reads as overstated for a 4-PR arc whose phase split is already visible in the dates. Capability-accretion-with-corrective-second-half is the register that fits the diff shape most directly; arc 04 and arc 05 are the precedents.

**Mandatory back-references (PR #8 and PR #10 to catalogue §4)**: arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) is the back-reference hub for catalogue §4 ("Generalized pager picker"). Arc 02 named PR #8 and PR #10 from the catalogue side as "parallel-but-different" — picker-shaped overlays that don't extend `PagerView::picker_items: Vec<(Label, Action)>`. Arc 05's PR #33 entry (= 01KR2AAX12XSNRNZPTXJT2TXJA) and PR #35 entry (= 01KR2AD5PV989H58E49E5D18NM) are the cross-arc parallel-pattern partners — both held DIRECTION ALIGNMENT with §4 from the pager-as-mode side; PR #8 and PR #10 hold PARALLEL PATTERN with §4 from the standalone-overlay side. Arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) and story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) named the cumulative reading: four PRs across two arcs hold §4 alignment; zero PRs execute the `PagerView::picker_items` shape; the deferral question goes to the insight layer.

PR #10's labeled-overlay shape might be expected to invoke catalogue §1 ("Numbered panels & direct-jump"), which the catalogue ranks **skip**. The PR #10 entry will refute honestly: PR #10's labels are alphabetic — a 23-letter alphabet `abcdefghilmnoprstuvwxyz` (skipping `q`/`Q` to leave the exit binding intact and `j`/`k` to spare reflexive vi motions), 1-letter when matches are few and 2-letter when many — not numeric, so §1's numbered-panels pattern is structurally not what PR #10 ships.

**Cross-thread back-link**: this thread continues from `history-overview` (segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P; PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK), arc 02 (investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T, the back-reference hub for catalogue §4), arc 03 (seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7, the `pane_focused`-three-meanings observation that the focus-axis branch of PR #25 rides), and arc 05 (framing = 01KR29ZCRYY132QKB0HKRRRERQ, PR #33 = 01KR2AAX12XSNRNZPTXJT2TXJA, PR #35 = 01KR2AD5PV989H58E49E5D18NM, story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB). Arc 06 follows arcs 01–05 in baseline-write order; arc 07 (`history-arc-07-codex-and-mcp-bridge`) is named at the closure entry for the next session.

The arc-content entries that follow this framing narrate PR #8, PR #10, PR #25, and PR #32 in arc order (which is also chronological within this arc). The closure entry forward-references arc 07. This thread remains OPEN for cross-arc references and the eventual Phase 3 insight layer.

Provenance:
- 62fc129 (PR #8 feat/harpoon, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02).
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06).
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06).
- `history-overview` framing entry = 01KR0TRFWT9W6WMFHC49YSW0BG (voice contract source).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-06 member-PR list).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract for arc 06's PR #8 / PR #10).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; back-reference hub).
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (`pane_focused`'s three meanings; focus-axis observation relevant to PR #25's chord-completion stamp).
- `history-arc-05-pager-surface` framing = 01KR29ZCRYY132QKB0HKRRRERQ (cadence A' precedent that arc 06 inherits but does not need at 4 PRs).
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (cross-arc DIRECTION ALIGNMENT partner for catalogue §4).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (cross-arc DIRECTION ALIGNMENT partner for catalogue §4).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- interim themes entry = 01KR2DYTPNCY5J5HPB99GT0J5M (pattern-8 / pattern-10 candidate diagnoses; arc 06 settles on capability-accretion-with-corrective-second-half).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state surface descriptions; `app/mod.rs` 9087-line dispatch surface).
- `onboarding-overview` entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE (front door).

<!-- Entry-ID: 01KR2G8042HWE419X0ESWKN205 -->

---
Entry: Claude Code (caleb) 2026-05-08T00:39:19.930445+00:00
Role: scribe
Type: Note
Title: PR #8 (feat/harpoon): per-project pinned working set, the new H chord prefix family, an =h filter that surfaces ancestor directories — picker-shaped overlay, parallel pattern with catalogue §4

Spec: scribe

tags: #history #arc-06

PR #8 opens phase α. Commit subject reads "harpoon: per-project pinned working set + =h filter (v1.39.0)" (commit 62fc129, 2026-05-02). The version bump is a minor cut (1.38.1 → 1.39.0), consistent with the user-visible feature scale. Diff: 14 files, 1,037 insertions / 22 deletions. The bulk lands in two files: `src/state/harpoon.rs` (367 insertions, new module) and `src/app/mod.rs` (481 insertions). `src/keymap/resolver.rs` gains 65 lines for the new chord prefix family; `src/keymap/action.rs` gains 10 lines for four new `Action` variants.

**The capability shipped.**

The `### Added` CHANGELOG entry reads verbatim: "Harpoon — per-project pinned working set. Inspired by ThePrimeagen's neovim plugin: a small (max 9), hand-curated, ordered list of file or directory pointers for muscle-memory navigation. `Ha` appends the cursor file/dir, `Hx` removes, `H1`..`H9` jumps to slot N (chdirs to the parent and places the cursor on the file; chdirs *into* the slot if it's a directory). `Hh` opens a modal menu where `K`/`J` reorder, `dd` deletes (vim-style two-key arming), `Enter`/`1`-`9` jumps. `=h` (or `:limit h`) filters the listing to harpoon entries plus all their ancestor directories — so `foo/` shows up when viewing `src/` and `src/foo/bar/hello.c` is harpooned, letting you drill in. Persisted at `$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml` per `PROJECT_HOME`; auto-saved on every mutation. Two PROJECT_HOMEs with the same basename can't collide (filename is keyed by an absolute-path hash)." (commit 62fc129, 2026-05-02).

The `### Changed` half pairs the `H` rebind: "`H` is no longer an alias for 'jump to `$HOME`'. It's now the harpoon chord prefix. The `~` key and the Home key still jump to `$HOME`; `gh` still jumps to `PROJECT_HOME`. This frees the natural `H1`..`H9` muscle-memory bindings without three-keystroke chord overhead." (commit 62fc129, 2026-05-02).

**The struct shape.**

PR #8 ships two structures, both standalone. The model lives in `src/state/harpoon.rs`:

```
pub struct Harpoon {
    pub slots: Vec<PathBuf>,        // index 0 = slot 1 (H1)
    pub project: PathBuf,           // PROJECT_HOME this list is scoped to
    ancestor_cache: HashSet<PathBuf>, // not persisted; rebuilt on mutation
}
```

The `slots` field is a flat `Vec<PathBuf>` with no `None` holes — empty slots are simply absent. The `MAX_SLOTS = 9` constant is named to "match `H1`..`H9` chord coverage." The `ancestor_cache` is the load-bearing data structure for the `=h` filter: every mutation rebuilds the set of slot paths plus all their ancestor directories, so `foo/` appears in the filtered listing when `src/foo/bar/hello.c` is harpooned and the user is browsing `src/`.

The overlay lives on `App` as a separate struct:

```
struct HarpoonMenu {
    cursor: usize,        // 0-based, indexes the active non-empty slots
    delete_armed: bool,   // vim-style dd: first d arms, second d deletes
}
```

`App` carries two new fields paired together: `harpoon: Option<Harpoon>` (active list, `None` when `PROJECT_HOME` is unset) and `harpoon_menu: Option<HarpoonMenu>` (active overlay, intercepts keys before normal dispatch when `Some`). The overlay is rendered as a centered modal `Block` with `Borders::ALL`, `Clear`-ed background, a title showing the project basename, slots displayed by 1-based index with paths shown relative to `project` when possible, and a footer line listing the bindings (`"j/k move · 1-9/Enter jump · K/J reorder · dd delete · q/Esc close"`).

**The dispatch interception.**

The overlay carries its own key handler. The `App::draw` path checks `if self.harpoon_menu.is_some() { self.render_harpoon_menu(frame); }` after the rest of the UI; the `App::handle_key` path checks `if self.harpoon_menu.is_some() { return Ok(self.handle_harpoon_menu_key(key)); }` before normal dispatch. Inside `handle_harpoon_menu_key`, the `delete_armed` two-key flag is captured and cleared at the top of the function, then the match arms re-set it on bare `d`. The doc-comment names the design choice explicitly: "The pending-d flag lives on App so it survives across this call (which can't borrow `menu` mutably across re-entry). Using a local approach: piggyback on `cursor`'s high bit would be hacky — keep it simple and use a separate bool field on `HarpoonMenu`."

**The `H` chord prefix family.**

`src/keymap/resolver.rs` gains a new `PendingSeq::Harpoon` variant alongside the existing `^a` (W), `]` (Bracket), `g` (G), etc. families. Top-level `H` triggers the prefix; the next key resolves to one of four actions:

```
KeyCode::Char(c @ '1'..='9') => ResolverOutcome::Action(Action::HarpoonJump(c as u8 - b'0')),
KeyCode::Char('a' | 'A')     => ResolverOutcome::Action(Action::HarpoonAppend),
KeyCode::Char('x' | 'X')     => ResolverOutcome::Action(Action::HarpoonRemove),
KeyCode::Char('h')           => ResolverOutcome::Action(Action::HarpoonOpenMenu),
```

The pre-existing `KeyCode::Char('H' | '~') | KeyCode::Home` arm that mapped `H` to `Home` becomes `KeyCode::Char('~') | KeyCode::Home` — `H` is freed as the new chord prefix. The diff adds a comment naming the rebind reason: "`~` and the Home key both still jump to `$HOME`. `H` was formerly an alias here but is now the harpoon chord prefix." Eight new resolver tests land alongside the change, including one that asserts the rebind: `feed(&mut r, key('H'))` now returns `ResolverOutcome::Pending` instead of `ResolverOutcome::Action(Action::Home)`.

The new chord family is consequential for arc 06's phase β. PR #32's CHANGELOG (commit a7867fb, 2026-05-06) names `H1`..`H9` explicitly among the chord families broken when user keybindings preempted the chord's second key. The harpoon family is among the eight chord prefixes (`^a`, `[`, `]`, `H`, `W`, `m`, `'`, `y`) PR #32 fixes by making chord-prefix-pending state win over user-keymap consultation; the per-PR PR #32 entry below names the rule against the diff. The connection from PR #8's chord introduction to PR #32's precedence rule is detectable from the diffs but not asserted in either commit.

**Persistence: ships in PR #8, not deferred.**

The `Harpoon::load` and `Harpoon::save` paths land in `src/state/harpoon.rs` alongside the model. The persistence layout is documented in the module-level doc-comment: "`$XDG_STATE_HOME/spyc/harpoon/<basename>.<hash>.toml`, one file per project keyed by `PROJECT_HOME`. Auto-saved on every mutation. The hash component is a 64-bit `DefaultHasher` digest of the absolute project path, hex-encoded; the `<basename>` prefix is a human-readable disambiguator. If `DefaultHasher` ever changes across Rust versions, users will see a fresh empty list rather than a corrupt one — the old file becomes an orphan."

The auto-save discipline lives at the call sites in `src/app/mod.rs`: every harpoon mutation (`harpoon_append`, `harpoon_remove`, `K`/`J` reorder, `dd` delete) calls `h.save()` and flashes an error on failure ("harpoon save failed: {e}"). The `App::apply` dispatcher is wrapped to call a `reconcile_harpoon()` helper after each action: when `state.project_home` shifts (from a chdir), the active harpoon is saved and a fresh one is loaded for the new project. The reconcile path also flips `harpoon` on/off when `PROJECT_HOME` is set/unset. BUGS.md's pre-existing SMALL entry naming "persistent across sessions" as a requirement is honored by PR #8's diff: persistence is not deferred to a follow-up.

**The PR #7 → PR #8 chronology, narrated from arc 04 already.**

Arc 04 owns the genesis side of harpoon. PR #7 (`feat/limit-git`, f3ddaf2, 2026-05-02) cut the `=git` / `=g` filter four hours before PR #8 merged, and the BUGS.md SMALL entry framed harpoon as deferred behind a "real design pass" — "design space overlaps existing concepts." PR #8's diff removes that BUGS.md entry verbatim:

```
-- harpoon-style "currently working on" pinned set — small ordered
-  per-project file list with quick numeric jumps, persistent across
-  sessions; not just a filter mode. Distinct from picks (per-dir,
-  ephemeral), marks (single-file pointer per letter), inventory
-  (yank stash). Needs a real design pass — design space overlaps
-  existing concepts. https://github.com/theprimeagen/harpoon
-  (`=git` / `=g` filter shipped separately for the simple "files in
-  git status" case.)
```

The four-hour gap and the deferred-design-pass framing are arc 04's territory and read from PR #7's vantage there. From PR #8's vantage in arc 06, the relevant observation is the structural one: PR #8 ships harpoon as four distinct surfaces — direct-jump chord (`H1`..`H9`), modal overlay (`Hh`), append/remove pair (`Ha`/`Hx`), and listing filter (`=h`) — sharing one `Harpoon` model, plus the persistence layer and the `reconcile_harpoon` swap-on-chdir behavior. The "design space overlaps existing concepts" the BUGS.md text named is settled by what the diff actually distinguishes: harpoon is per-project (vs. picks: per-dir, ephemeral; marks: single-file per letter; inventory: yank stash). The four primitives stay distinct.

**Catalogue §4 alignment — parallel pattern, not direct execution.**

Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) frames the catalogue §4 disposition for arc 06's phase α: "Arc 06's PR #8 (`feat/harpoon`) and PR #10 (`feat/quickselect`) ship picker-shaped overlays; the diff shape suggests both ship as standalone overlays rather than as the `pager.picker_cursor` extension §4 recommends. The catalogue's intended pattern lives, parallel-but-different, alongside the executed work." That disposition holds against the PR #8 diff.

Catalogue §4's specific recommendation (per arc 02's investigation entry, quoting the catalogue): "extend the pager into a generalized pick-from-list mode" via "a `PagerView::picker_items: Vec<(Label, Action)>` field with Enter-to-fire dispatch." PR #8's `HarpoonMenu` is structurally a different shape: the menu is a `Block`-and-`Borders` modal rendered by `App::render_harpoon_menu`, with its own cursor and `delete_armed` state, dispatched through `App::handle_harpoon_menu_key`, which intercepts keys before normal dispatch. The pager is not involved. There is no `PagerView::picker_items: Vec<(Label, Action)>` field; the picker shape lives on `App` rather than on `PagerView`.

Two further structural notes against §4's pattern:
- Enter-to-fire is not the primary dispatch shape. The primary surfaces are direct: `H1`..`H9` jumps without entering the menu at all (the chord resolves directly to `Action::HarpoonJump(N)` via the resolver), and `=h` filters the listing without any picker. The modal overlay is a *secondary* surface for reorder/delete; jumping by Enter is one of several bindings inside it (alongside `1`..`9`).
- The action set is closed and harpoon-specific. `HarpoonMenu`'s bindings (`j`/`k`/`g`/`G`/`K`/`J`/`Enter`/`1-9`/`dd`/`Esc`/`q`) are hard-coded for the harpoon use-case; §4's pattern envisions `Vec<(Label, Action)>` as a generalized substrate any list-of-options surface could populate. The harpoon menu does not generalize.

The arc-02 disposition holds: **PARALLEL PATTERN**, not direct execution of §4. This entry back-references arc 02's investigation entry to confirm the disposition against the diff.

Arc 05's PR #33 entry (= 01KR2AAX12XSNRNZPTXJT2TXJA) and PR #35 entry (= 01KR2AD5PV989H58E49E5D18NM) are the cross-arc parallel-pattern partners. Both held DIRECTION ALIGNMENT with §4 from the pager-as-mode side: PR #33's `VisualSelection` field on `PagerView` is structurally analogous to `picker_items` (both are `Option`-shaped state on `PagerView` that gates a mode), but selects a line range rather than picking from a list; PR #35's `display_in_pane` launches an external `$PAGER` into a top overlay rather than populating spyc's internal `PagerView`. PR #8 holds parallel pattern with §4 from the standalone-overlay side — same family of "the picker shape, but not as a pager mode."

Arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) named the cumulative reading after arc 05: four PRs across two arcs hold §4 alignment; zero execute the `PagerView::picker_items` shape. Arc 06 carries that reading forward without resolving it; the deferral question goes to the insight layer per arc 05's story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB).

**Drift findings flagged for the insight layer**:

- The `H1`..`H9` direct-jump path bypasses any picker entirely — the chord resolves directly to `Action::HarpoonJump(N)` without opening the modal overlay. The "picker" framing in this entry is therefore partial: PR #8's primary surface for slot recall is single-keystroke direct dispatch, not picker-from-list. Whether that argues against §4's picker-pattern relevance for harpoon's *primary* use-case (and §4 stays load-bearing only for the secondary modal-menu use-case) is for the insight layer.
- The hash-keyed persistence file (`<basename>.<hash>.toml` with `DefaultHasher` digest) trades portability for collision avoidance. The doc-comment notes that `DefaultHasher` is not stable across Rust versions ("If `DefaultHasher` ever changes across Rust versions, users will see a fresh empty list rather than a corrupt one — the old file becomes an orphan."). Captured for the insight layer's portability/persistence reading if relevant.
- The `ancestor_cache` recomputation on every mutation is the cheap-rebuild discipline arc 05's PR #11 entry (= 01KR2A121DSV81GM4EBCKAVAAM) used for `last_body_w` on `PagerView` and arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) used at overlay-vs-pane boundaries: "small contained corrections that recompute over preserve a complex invariant." Same shape, different surface.

Provenance:
- 62fc129 (PR #8 feat/harpoon, 2026-05-02).
- `git diff 62fc129^1..62fc129^2 -- CHANGELOG.md`: `### Added` and `### Changed` entries quoted verbatim above.
- `git show 62fc129^2:src/state/harpoon.rs` (367 lines, new module): `Harpoon` struct definition; `MAX_SLOTS = 9` constant; module-level doc-comment naming persistence layout, `DefaultHasher` portability caveat, and "navigate first, decide second" semantics.
- `git diff 62fc129^1..62fc129^2 -- src/app/mod.rs`: `HarpoonMenu` struct (cursor, delete_armed); `harpoon: Option<Harpoon>` and `harpoon_menu: Option<HarpoonMenu>` fields on `App`; `render_harpoon_menu` body; `handle_harpoon_menu_key` body (quoted in part above); `apply` wrapping with `reconcile_harpoon` post-hook.
- `git diff 62fc129^1..62fc129^2 -- src/keymap/resolver.rs`: `PendingSeq::Harpoon` variant; `H` chord prefix entry; `~`/Home arm for `Action::Home` retained without `H`; eight new test cases including the `H` rebind regression.
- `git diff 62fc129^1..62fc129^2 -- src/keymap/action.rs`: four new `Action` variants — `HarpoonJump(u8)`, `HarpoonAppend`, `HarpoonRemove`, `HarpoonOpenMenu`.
- `git diff 62fc129^1..62fc129^2 -- BUGS.md`: SMALL entry naming "harpoon-style 'currently working on' pinned set" removed (8 lines).
- `Cargo.toml:3` post-merge: `version = "1.39.0"` (1.38.1 → 1.39.0 minor cut).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; PR #8 named at this entry as "parallel-but-different" / parallel pattern).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract: PR #8 / PR #10 to arc 02 mandatory).
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (cross-arc DIRECTION ALIGNMENT partner — pager-as-mode side of the §4 alignment family).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (cross-arc DIRECTION ALIGNMENT partner — pager-launchable-from-listing side of the §4 alignment family).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05; arc 06 carries it forward).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- `history-arc-04-git-integration` (PR #7 entry — chronology and BUGS.md genesis side already narrated there; arc 06 narrates from PR #8's own vantage).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-vs-pane focus model precedent).
- `history-arc-05-pager-surface` PR #11 entry = 01KR2A121DSV81GM4EBCKAVAAM (cheap-rebuild discipline precedent for `ancestor_cache`).
- `history-arc-06-input-and-overlays` framing entry = 01KR2G8042HWE419X0ESWKN205.

<!-- Entry-ID: 01KR2GCH3Q8DR9DATBBC802Q8W -->

---
Entry: Claude Code (caleb) 2026-05-08T00:41:47.627253+00:00
Role: scribe
Type: Note
Title: PR #10 (feat/quickselect): ^a u labeled-overlay picker for pane output, alphabetic labels with case-as-intent — picker-shaped overlay, parallel pattern with §4, refutation against §1

Spec: scribe

tags: #history #arc-06

PR #10 is the second move in phase α, landing 2 hours and 48 minutes after PR #8 the same calendar day. Commit subject reads "quick select: ^a u labeled-overlay picker for pane output (v1.40.0)" (commit 9043547, 2026-05-02). The version bump is a second minor cut on the same day (1.39.0 → 1.40.0); the rapid sequential minor cuts read as feature-shipped-feature-shipped, not coalesced. Diff: 15 files, 985 insertions / 15 deletions. The bulk lands in two files: `src/pane/quick_select.rs` (436 insertions, new module) and `src/app/mod.rs` (331 insertions). `src/pane/mod.rs` gains 24 lines (the new `pickable_text` helper).

**The capability shipped.**

The `### Added` CHANGELOG entry reads verbatim (the relevant block in full): "Quick Select — labeled overlay picker (`^a u`). Borrowed from WezTerm's mode of the same name. Press `^a u` to scan the visible pane for URLs, file paths, git SHAs, IPv4 addresses, and any user-defined regex patterns; each match is overlaid with a 1- or 2-letter label. Lowercase label → yank to clipboard. **Uppercase label → 'open' intent**, dispatched per match kind: URLs → system handler (`open` / `xdg-open`); Paths → cursor-jump in spyc; Git SHAs → `git show <sha>` in the in-app pager; Custom patterns with a `url = 'https://.../{}'` template → fill `{}` with the match, then `open`/`xdg-open`; Other kinds → fall back to yank with a flash hint. Scroll mode 'just works': the picker scans exactly the user's visible viewport, so scrolling up to a Claude reply and pressing `^a u` labels the URLs in *that* reply." (commit 9043547, 2026-05-02).

The `### Fixed` half is a side-fix bundled under the same PR: "`gf` / `gF` now honor scroll mode. Previously `goto_file_from_pane` temporarily forced scrollback to its deepest position, so a path the user had scrolled up to was ignored — the scanner read a different region of history. Now routes through a new `Pane::pickable_text()` helper: when scrolling, scans exactly the visible viewport; when live, the prior 200-line behavior is preserved so paths in large diffs that just scrolled past the bottom are still findable." (commit 9043547, 2026-05-02). The fix shares infrastructure with the new feature — the `pickable_text` helper exists primarily to feed quick select but is also wired into the path-scanner that `gf`/`gF` use.

**The struct shape.**

The picker model lives in `src/pane/quick_select.rs`:

```
pub struct QuickSelect {
    pub matches: Vec<Match>,
    pub pending_first: Option<char>,  // first key buffered when labels are 2-letter
    pub all_two_letter: bool,         // true iff every label is 2-letter (>23 matches)
    pub open_intent: bool,            // sticky bit for uppercase-first in 2-letter case
}
```

`Match` is structured per match: `text`, `kind: MatchKind`, `label`, `row`, `col`. `MatchKind` is an enum: `Url`, `Path`, `GitSha`, `Ipv4`, `Custom { name: String, url_template: Option<String> }`. The custom variant carries the `url_template` so dispatch can fill `{}` with the matched text and hand off to `open`/`xdg-open`; without a template, custom matches fall back to yank.

The struct lives on `App` as `quick_select: Option<QuickSelect>` (active when the mode is open; `None` when closed), and the dispatch path checks for it before normal pane-key handling. The doc-comment names a concrete behavioral choice: "the snapshot of visible text isn't retained — we extract matches with their coordinates and let the live pane keep rendering underneath. If the pane scrolls during the mode the labels go stale; the simplest correct behavior is to close the picker on the next event tick that detects pane growth."

**The alphabet — and what it deliberately excludes.**

`src/pane/quick_select.rs` defines `const ALPHABET: &[u8] = b"abcdefghilmnoprstuvwxyz";` — 23 letters, with three deliberate omissions:

```
/// Reserved keys we mustn't generate as labels: `q`/`Q` exits the
/// mode; the alphabet skips both. Keeping it lowercase-only keeps
/// the uppercase forms free for the "open" intent (`A` opens `a`,
/// etc.). We also skip `j`/`k` so that someone who reflexively
/// reaches for vi motions during the mode just gets ignored input
/// instead of an accidental action.
```

`q` and `Q` are reserved for the mode-exit binding, so they're omitted from the label generator (otherwise pressing the exit key would trigger an action instead of exiting). `j` and `k` are omitted so vi-reflex motions during the mode produce no-op input rather than firing whichever match happened to land on those labels. The reasoning is preserved verbatim in the source comment.

Label assignment is in `assign_labels`: 1-letter labels when `matches.len() <= ALPHABET.len()` (23 or fewer matches), otherwise 2-letter labels. The hard cap is `MAX_MATCHES = ALPHABET.len() * ALPHABET.len()` = 529; viewport scans yielding more matches than that truncate to the cap with the rationale: "they're the most likely to scroll past anyway, and a forest of 3-letter labels would be unreadable."

**The case-as-intent dispatch.**

The mode interprets keystrokes by case. Lowercase = yank: pressing the label letters yanks the matched text to the clipboard via `pbcopy` and exits. Uppercase = "open" intent, dispatched per match kind: URL → system handler (`open` / `xdg-open`); Path → cursor-jump in spyc; GitSha → `git show <sha>` into the pager; Custom with `url_template` → format the URL and hand off to the system handler; other kinds → fall back to yank with a flash hint naming the kind.

The 2-letter case adds the `open_intent` sticky bit: an uppercase first keystroke preserves the open-intent across the second keystroke, so `Ab` opens (same as `aB` or `AB`). In the 1-letter case the sticky bit is unused — the single keystroke commits directly with case-as-intent.

**The match kinds and built-in patterns.**

Five kinds. The built-in regexes (in `build_patterns` order, which determines overlap precedence — earlier patterns win on overlap):

- `URL_PATTERN`: `r"https?://\S+"`. Trailing punctuation (`.`, `,`, `;`, `:`, `!`, `?`, `]`, `}`, `>`) is trimmed via `trim_url`; parens are deliberately *not* trimmed because "many real URLs have balanced parens (Wikipedia, MSDN). This errs on capturing slightly too much rather than silently dropping a URL char."
- `GIT_SHA_PATTERN`: `r"\b[0-9a-f]{7,40}\b"`. Lower bound is git's `--short` default; upper bound rules out 64-hex SHA-256 hashes "that aren't SHAs (commonly seen in Cargo.lock checksums)."
- `IPV4_PATTERN`: `r"\b\d{1,3}(?:\.\d{1,3}){3}\b"`. Doesn't validate octet ranges ("would just refuse 256.256.256.256 etc., which never appear in real output anyway").
- Custom patterns: user-defined via `[[scan.patterns]]` in `.spycrc.toml`, with `name`, `regex`, optional `url`. Bad regexes are dropped at config load with a debug-log note; "one typo never blocks startup."
- `PATH_PATTERN`: `r"[\w./~][\w./~+\-]*/[\w./~+\-]+"`. Last in the precedence order (broadest, most likely to over-match other things).

The regex compile happens once at config load via `build_patterns`; `RegexSet` is deliberately not used because "it tells us *which* regexes matched but not *where*, and we need the spans to place labels."

**The `pickable_text` helper and the `gf`/`gF` side-fix.**

`src/pane/mod.rs` gains a new method on `Pane`:

```
pub fn pickable_text(&mut self, recent_n: usize) -> Vec<String> {
    if self.is_scrolling() {
        self.visible_lines()
    } else {
        self.recent_lines(recent_n)
    }
}
```

The doc-comment names the design contract: "Text that interactive pickers (`gf`/`gF`, `^a u`) should scan: what the user is currently looking at. While scrolling, that is the exact visible viewport at the user's scroll position; while live, we widen to the last `recent_n` lines so paths/URLs that just rolled past the bottom are still findable. Without this distinction, scanning from a fixed slice means a user who scrolled up to find a URL would have it ignored — the picker would read a different region than their eyes."

The helper has two callers in PR #10's diff: the new quick-select scanner and the rerouted `gf`/`gF` path. The pre-existing `goto_file_from_pane` had a bug — it temporarily forced scrollback to its deepest position, breaking path-scan in scroll mode. The fix routes through `pickable_text` instead, getting "follows the user's eye" for free.

The side-fix is bundled with the feature because they share infrastructure. From PR #10's vantage the two halves read as one structural move (introduce `pickable_text` as the contract for "what the picker should scan") with two consumers; from outside, the bundling is the kind of thing the eventual insight layer's drift catalogue might note (a `feat:` PR carrying a `### Fixed` block).

**Catalogue §4 alignment — parallel pattern, not direct execution.**

Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) names PR #10 alongside PR #8 as "parallel-but-different" — picker-shaped overlays that don't extend `PagerView::picker_items`. That disposition holds against the diff.

Three structural distances from §4's specific shape:
- The picker structure lives on `Pane`-adjacent code, not on `PagerView`. `QuickSelect` is in `src/pane/quick_select.rs`, alongside `pane/input.rs` and `pane/widget.rs`; it is co-located with the rest of the pane code because, per the module doc-comment, "Quick Select is only meaningful when there's a pty pane to scan, and its input is the pane's visible text grid." `PagerView` is not involved.
- The dispatch shape is case-as-intent, not Enter-to-fire. Lowercase-vs-uppercase dispatches to two different intents (yank vs open) per single keystroke. §4's pattern envisions Enter as the dispatch trigger over a `Vec<(Label, Action)>` substrate. Quick select's dispatch is per-label-keystroke, not per-Enter.
- The action substrate is closed and kind-typed. `Match::kind` is an enum (`Url` / `Path` / `GitSha` / `Ipv4` / `Custom { url_template }`), and the open-intent dispatch is a switch on the kind — not a generalized `Action` slot. §4's `Vec<(Label, Action)>` envisions the substrate as parametric over any `Action`; quick-select dispatches based on what kind of thing was matched, not what action was attached.

The arc-02 disposition holds: **PARALLEL PATTERN**, not direct execution of §4. This entry back-references arc 02's investigation entry to confirm the disposition against the diff.

Cross-arc parallel-pattern partners are the same as for PR #8: arc 05's PR #33 entry (= 01KR2AAX12XSNRNZPTXJT2TXJA, DIRECTION ALIGNMENT) and PR #35 entry (= 01KR2AD5PV989H58E49E5D18NM, DIRECTION ALIGNMENT) hold §4 alignment from the pager-as-mode side; PR #8 and PR #10 hold parallel pattern from the standalone-overlay side. Arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) and story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) carry the cumulative reading: four PRs, two arcs, zero `picker_items` execution.

**Catalogue §1 disposition — refutation, honestly.**

Catalogue §1 ("Numbered panels & direct-jump") could be expected to apply to a labeled-overlay picker; the framing entry flagged the question. Reading PR #10's diff against §1's shape: the labels are alphabetic (`a`..`z` minus `q`/`j`/`k`), not numeric. The catalogue's §1 pattern is specifically lazygit's `[N]-Status`, `[N]-Files` etc. with `1`..`5` as direct-jump targets to top-level panels; the catalogue ranks §1 **skip** for spyc on the grounds that "spyc has exactly two top-level surfaces (list, pane) where lazygit has five, so `1` and `2` would be wasted on a binding that `^W j`/`^W k` already covers cleanly. The `[N+]`/`[N●]` task-divider glyphs (DESIGN.md) already use single digits in titles for *task* numbers; hijacking `1`..`9` globally would collide."

PR #10 doesn't ship numbered direct-jumps to anything. The labeled-overlay picker is structurally a different idiom: ephemeral 1- or 2-letter labels assigned per match per scan, alphabetic alphabet, case-as-intent dispatch. The §1 SKIP recommendation is not invalidated by PR #10's existence; PR #10 is not a §1 instance. The relevant catalogue cross-reference for PR #10 is §4, not §1.

(One minor surface where digits *do* reach the pane via PR #10: the GitSha pattern matches 7-40 hex characters, including digits. But that's regex content matching, not numeric-direct-jump labels in the §1 sense.)

**Drift findings flagged for the insight layer**:

- The `feat/quickselect` PR ships a `### Fixed` half (`gf`/`gF` scroll-mode honoring) bundled under the feature slug. The bundling pattern recurs in this arc: PR #25 below also bundles a defensive fix with diagnostic infrastructure. Captured for the eventual insight layer's bundle-shape catalogue. Arc 04's PR #15 entry already named one such bundle from the git side (a pane-control fix bundled with a git-marker leak fix); the bundle pattern is recurring, not one-off.
- The `pickable_text` helper is the kind of contract-introducing shape that becomes structural infrastructure when a third caller arrives — quick-select and `gf`/`gF` are two callers; whether a future picker (a hypothetical `^a o` for OSC-8 hyperlinks, etc.) joins or whether `pickable_text` stays at two consumers is determinable only outside the 22-day window.
- The "labels go stale on scroll" close-on-pane-growth behavior named in the doc-comment is a specific design choice with alternatives (re-scan on growth, freeze the snapshot, scroll-lock the pane). Whether the close-on-growth choice gets revisited is fuel for the insight layer if a recurrence shows up.

Provenance:
- 9043547 (PR #10 feat/quickselect, 2026-05-02).
- `git diff 9043547^1..9043547^2 -- CHANGELOG.md`: `### Added` and `### Fixed` entries quoted verbatim above.
- `git show 9043547^2:src/pane/quick_select.rs` (436 lines, new module): `QuickSelect` struct definition; `MatchKind` enum (Url/Path/GitSha/Ipv4/Custom); `ALPHABET` constant with reserved-keys doc-comment quoted verbatim above; `assign_labels` implementation and 1-vs-2-letter logic; `build_patterns` overlap-precedence ordering; `URL_PATTERN`, `GIT_SHA_PATTERN`, `IPV4_PATTERN`, `PATH_PATTERN` constants.
- `git diff 9043547^1..9043547^2 -- src/pane/mod.rs`: new `pickable_text` method on `Pane` (quoted in full above); `quick_select` module declaration.
- `git diff 9043547^1..9043547^2 -- src/keymap/action.rs`: new `Action::QuickSelectOpen` variant with comment "^a u — scan visible pane, label matches, pick to yank/open".
- `git diff 9043547^1..9043547^2 -- src/keymap/resolver.rs`: `^a` chord prefix gains `KeyCode::Char('u' | 'U') => ResolverOutcome::Action(Action::QuickSelectOpen)` arm.
- `git diff 9043547^1..9043547^2 -- BUGS.md`: SMALL entry "we should steal some ideas from https://wezterm.org/ e.g. simple visual picker for urls / id's from the terminal view" removed (2 lines).
- `git diff 9043547^1..9043547^2 -- src/config/mod.rs`: 49 lines added for `[[scan.patterns]]` config schema.
- `Cargo.toml:3` post-merge: `version = "1.40.0"` (1.39.0 → 1.40.0 minor cut, two minor cuts on the same calendar day).
- `notes/lazygit-ux-catalogue.md` §1 (skip recommendation; numbered-panels rationale "spyc has exactly two top-level surfaces" / "hijacking `1`..`9` globally would collide" — quoted verbatim above), at `git show 0691666:notes/lazygit-ux-catalogue.md`.
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; PR #10 named at this entry as "parallel-but-different" / parallel pattern).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract: PR #8 / PR #10 to arc 02 mandatory).
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (cross-arc DIRECTION ALIGNMENT partner — pager-as-mode side).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (cross-arc DIRECTION ALIGNMENT partner — pager-launchable-from-listing side).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- `history-arc-04-git-integration` PR #15 entry (bundle-shape precedent for bundled fix-with-feature; arc 04's narration of the pane-control + git-marker leak bundle).
- `history-arc-06-input-and-overlays` framing entry = 01KR2G8042HWE419X0ESWKN205.
- `history-arc-06-input-and-overlays` PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W (sibling phase-α entry; same parallel-pattern disposition with §4).

<!-- Entry-ID: 01KR2GH1D9QCGDPZEMWW09R898 -->

---
Entry: Claude Code (caleb) 2026-05-08T00:43:50.644397+00:00
Role: scribe
Type: Note
Title: PR #25 (fix/input-dispatch-hardening): two defensive guards under one bug, plus a --key-trace diagnostic switch user-visible at the CLI

Spec: scribe

tags: #history #arc-06

PR #25 opens phase β. Commit subject reads "fix: input dispatch hardening + --key-trace diagnostic switch (v1.41.12)" (commit bfc4a18, 2026-05-06). Diff: 7 files, 205 insertions / 4 deletions. Bulk lands in `src/app/mod.rs` (83 insertions) and a new `src/key_trace.rs` module (64 insertions); `src/main.rs` adds 11 lines for the CLI wiring.

**The bundle, named at the slug.**

PR #25's title is unusually explicit about the bundling — `fix: <hardening> + <diagnostic switch>` — and the diff shape matches the slug literally. Two distinct hardening guards plus one diagnostic infrastructure module under one PR. The hardening half is multi-staged in the sense that two distinct code paths are touched, but the staging is enumeration (two named cases under one user report), not a numbered ladder of progressively-broader fixes.

**The user report and the "couldn't reproduce" framing.**

BUGS.md's `### FIXED ###` block names the report verbatim and the response shape:

```
(defensive, v1.41.12) "switching panes input doesn't work when done
too quickly" — couldn't reproduce, but two plausible failure modes
were addressed: (1) post-chord bounce: a focus-switch chord
(`^a-j`/`^a-k`) now suppresses a same-key Press/Repeat within 60 ms
so a fast chord doesn't leak a stray byte into the just-focused
pane; (2) stranded paste: `Event::Paste` outside Prompting / with
no pane open now flashes "paste ignored" instead of silently
dropping. Also added `--key-trace`/`SPYC_KEY_TRACE` for diagnosing
the next report — writes every event + dispatch decision to
`/tmp/spyc-key-trace-<ts>.log` with elapsed-since-start timestamps.
```

The framing is honest: "couldn't reproduce, but two plausible failure modes were addressed." The fix is defensive, not a confirmed root-cause repair. The diagnostic switch ships *with* the defensive fix specifically so the next report comes with a reproduction log.

**Hardening case 1: post-chord bounce-suppression.**

`src/app/mod.rs` adds a new field on `App`:

```
focus_chord_completed: Option<(std::time::Instant, KeyCode)>,
```

The doc-comment names the failure mode: "When a focus-switch chord (^a-j / ^a-k) just completed, this captures (when, the key that completed it). The next dispatch drops any Press/Repeat of the same key within ~60 ms — without this guard, fast typing of `^a-j-...` produced a stray `j` byte to the now-focused pane child (the `j` Press completes the chord, but a brief OS-level Repeat or a too-quick second Press would otherwise arrive with the new focus already active)."

The dispatch path adds two pieces. The chord-completing path stamps the field when `Action::PaneFocusDown` or `Action::PaneFocusUp` resolves:

```
if matches!(action, Action::PaneFocusDown | Action::PaneFocusUp) {
    self.focus_chord_completed = Some((std::time::Instant::now(), key.code));
}
```

The next dispatch checks the field at the top of `handle_key`:

```
if let Some((at, code)) = self.focus_chord_completed {
    let within_window = at.elapsed() < Duration::from_millis(60);
    if within_window
        && key.code == code
        && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        && key.modifiers.is_empty()
    {
        crate::key_trace::log("  swallowed (post-chord bounce)");
        return Ok(PostAction::None);
    }
    if !within_window {
        self.focus_chord_completed = None;
    }
}
```

The 60ms window is named in the doc-comment: "60 ms covers system-key-repeat (~30-50 ms) and kitty-keyboard Repeat events without affecting deliberate double-taps." Modifiers-empty check filters to bare keypresses (so a deliberate `^a-j` after the chord completes still fires; only a bare `j` is swallowed).

The interaction with arc 03's focus axis is structural but indirect. Arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) named that `pane_focused: bool` post-PR-#34 carries three different meanings (list-vs-pane, zoom save-restore source, overlay-vs-pane). PR #25 does not change `pane_focused` or its meanings; it adds a sibling state field (`focus_chord_completed`) that fires specifically on the chord-driven focus-switch path. The chord-driven focus-switch is one branch of the bool's behavior — the same branch arc 03's PR #34 entry (= 01KR10JBACRS3Z71WTHGBVCPJM) named as the overlay-vs-pane axis — but PR #25 corrects a leak in the *transition*, not the *meaning*. The seam arc 03 named is unchanged.

**Hardening case 2: stranded-paste flash.**

`src/app/mod.rs`'s `Event::Paste` handler had two branches before PR #25: route to `state.mode` if `Mode::Prompting`, route to the active pane otherwise. PR #25 adds a third branch — the fall-through case where the user is neither prompting nor has a pane open:

```
} else {
    // No prompt and no pane — there's nowhere
    // sensible to send the paste. Some terminals
    // wrap rapid-fire keystrokes in bracketed
    // paste sequences, so silently dropping
    // could swallow real input. Flash a hint so
    // the user knows it happened.
    let n = text.chars().count();
    self.state.flash_info(format!(
        "paste ignored ({n} chars) — open `:` or `^\\` to paste"
    ));
}
```

The hypothesis the diff bakes in: some terminals wrap rapid-fire keystrokes in bracketed paste sequences, which the previous code silently dropped. The flash is the visible signal that an input arrived but had no destination.

**The `--key-trace` diagnostic — user-visible at the CLI.**

`src/main.rs` adds a new CLI argument:

```
/// Trace every key event + dispatch decision to
/// /tmp/spyc-key-trace-<ts>.log. Useful for diagnosing
/// "input doesn't work when done too quickly" reports.
/// Equivalent to setting SPYC_KEY_TRACE=1.
#[arg(long)]
key_trace: bool,
```

The `main` function calls `key_trace::init(cli.key_trace)` after `debug_log::init`, with a startup banner: `eprintln!("spyc: key trace → {p}");` when active. The flag is opt-in, off by default; mirrors the pre-existing `--debug` / `SPYC_DEBUG` pattern.

The new `src/key_trace.rs` module (64 lines, full content readable in one screen) provides three functions: `init(flag) -> Option<String>` (called once at startup; returns the log path when active), `is_enabled() -> bool` (cheap-guard for callers to skip expensive formatting when disabled), and `log(msg: &str)` (writes one line with elapsed-since-start ms prefix). The implementation uses a single `Mutex<Option<TraceState>>` static and `OpenOptions::create+append` on the log file at `/tmp/spyc-key-trace-<epoch_secs>.log`.

The trace points: `handle_key` entry logs `"RX kind={:?} code={:?} mods={:?} pane_focused={} pending={:?}"`; the resolver outcome logs `"  resolver -> {outcome:?}"`; the paste path logs `"RX paste len={} pane_focused={} mode={:?}"`; the bounce-suppression swallow path logs `"  swallowed (post-chord bounce)"`. The trace is per-key dispatch evidence, not a sampled metric — every event annotated with elapsed time and the dispatch decision.

The doc-comment names the diagnostic intent explicitly: "The intent is diagnostic-only: when a user reports an input bug ('typed ^a-j too fast and the focus didn't switch'), they can flip the flag, reproduce, and ship the log." The infrastructure ships in advance of the next report; there is no current consumer of the trace beyond the immediate one — PR #25 is the bundle that puts both the defensive fix and the diagnostic for *future* defensive fixes into the codebase at once. The connection between the "couldn't reproduce" framing and the diagnostic shipping at the same time is the load-bearing observation: PR #25 is acknowledging that next time, the report should come with a log.

**The two-cases-under-one-symptom shape.**

The user report ("switching panes input doesn't work when done too quickly") is one symptom; the fix is two distinct hypotheses about the symptom's cause, both addressed defensively. Hypothesis 1 (bounce) addresses the focus-switch case directly: a chord-driven focus switch was leaving a stray byte. Hypothesis 2 (stranded paste) addresses a different mechanism: bracketed paste might have been silently swallowing keystrokes the user typed too fast. Either could explain "input doesn't work when done too quickly" depending on terminal and keyboard timings; PR #25 ships both, plus the diagnostic to disambiguate next time.

The shape is not a plan-supersession ladder (no superseded fix exists; this is the first fix attempt). It's not a single-rule fix either (two distinct code paths change). It's an enumerated-cases bundle: two named cases under one symptom, addressed defensively, with diagnostic infrastructure for next time.

**Drift findings flagged for the insight layer**:

- The `--key-trace` infrastructure ships with no immediate consumer beyond the bug PR #25 already addresses. Whether it gets used in a future "input doesn't work" report is determinable only outside the 22-day window. The shape — *ship the diagnostic with the defensive fix so the next report is reproducible* — is fuel for the insight layer's future-proofing catalogue.
- The bundle-pattern (defensive fix + diagnostic infrastructure under one PR slug) recurs in this arc: PR #10 above bundles a `### Fixed` half (`gf`/`gF` scroll-mode honoring) under a `feat/` slug. The bundle pattern is recurring across arcs (arc 04's PR #15 entry already named one); the eventual insight layer can decide whether the bundling is fuel for drift or for the recurrence catalogue.
- The chord-completing-key stamp specifically fires only on `Action::PaneFocusDown | Action::PaneFocusUp`. Other chord families (`H`, `]`, `y`, `m`, `'`, `W`, `[`) don't mark the chord-completing key, so a hypothetical "post-chord bounce" on those families would not be caught by PR #25's guard. Whether the focus-switch chord is uniquely susceptible to the bounce (because it's the only chord that changes which child consumes raw bytes) or whether other chord families have latent equivalents is determinable only with `--key-trace` data from the field.

Provenance:
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06).
- `git diff bfc4a18^1..bfc4a18^2 -- CHANGELOG.md`: `### Fixed` (Input dispatch hardening for fast typing) and `### Added` (--key-trace / SPYC_KEY_TRACE) blocks quoted in part above.
- `git diff bfc4a18^1..bfc4a18^2 -- BUGS.md`: `### FIXED ###` entry "(defensive, v1.41.12)" quoted verbatim above.
- `git diff bfc4a18^1..bfc4a18^2 -- src/app/mod.rs`: `focus_chord_completed: Option<(std::time::Instant, KeyCode)>` field on `App` with full doc-comment; `handle_key` swallow guard quoted above; `Event::Paste` fall-through branch quoted above; chord-completion stamp on `PaneFocusDown`/`PaneFocusUp`.
- `git show bfc4a18^2:src/key_trace.rs` (64 lines, new module): `init`/`is_enabled`/`log` API; `Mutex<Option<TraceState>>` static; `OpenOptions::create+append` on `/tmp/spyc-key-trace-<epoch_secs>.log`; module doc-comment naming diagnostic intent.
- `git diff bfc4a18^1..bfc4a18^2 -- src/main.rs`: `--key-trace` CLI argument with `SPYC_KEY_TRACE` env var equivalence; `key_trace::init(cli.key_trace)` startup call; `eprintln!("spyc: key trace → {p}")` banner.
- `Cargo.toml:3` post-merge: `version = "1.41.12"`.
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (`pane_focused`-three-meanings observation; PR #25 corrects the chord-driven focus-switch transition without touching the bool's three meanings).
- `history-arc-03-pane-behavior` PR #34 entry = 01KR10JBACRS3Z71WTHGBVCPJM (overlay-vs-pane focus axis; PaneFocusDown/PaneFocusUp are the chord-driven actions PR #34 routed through the overlay-vs-pane axis).
- `history-arc-04-git-integration` PR #15 entry (bundle-pattern precedent; pane-control fix bundled with git-marker leak fix).
- `history-arc-06-input-and-overlays` framing entry = 01KR2G8042HWE419X0ESWKN205.
- `history-arc-06-input-and-overlays` PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898 (bundle-pattern precedent within this arc; `gf`/`gF` fix under a `feat/` slug).

<!-- Entry-ID: 01KR2GMSNX29CWFN154QBK6TJ3 -->

---
Entry: Claude Code (caleb) 2026-05-08T00:45:22.054388+00:00
Role: scribe
Type: Note
Title: PR #32 (fix/chord-priority-over-user-keymap): single rule — when a chord prefix is pending, the next key resolves the chord; the g chord stays the deliberate exception

Spec: scribe

tags: #history #arc-06

PR #32 closes phase β. Commit subject reads "fix: chord prefixes beat user keybindings on the second key (v1.41.19)" (commit a7867fb, 2026-05-06). Diff: 4 files, 145 insertions / 7 deletions. The bulk lands in `src/keymap/resolver.rs` (133 insertions, 2 deletions), with eight new test cases alongside the change.

**The capability shipped, and the bug behind it.**

The `### Fixed` CHANGELOG entry reads verbatim: "Built-in chord prefixes now beat user keybindings on the second key. A user reported `^a-n` / `^a-p` flashing the pending indicator and then doing nothing — they had `n` / `p` bound elsewhere in `.spycrc`, and the resolver was consulting user bindings *before* checking whether a chord was already in flight. Same root cause for `]g` / `[g` (anyone with `g` user-bound), `H1`..`H9`, `yp` / `yf` / etc., `ma`..`mz`, `'a`..`'z`, `Wl` / `Wn` / `Wd`. The fix flips the precedence: when an explicit chord prefix (`^a`, `[`, `]`, `H`, `W`, `m`, `'`, `y`) is pending, the next key resolves the chord. The `g` chord keeps its previous behavior — bare `g` is also a vi motion fragment users may want to remap (`gd` / `gf` / etc. remain user-overridable). Top-level user bindings are unaffected." (commit a7867fb, 2026-05-06).

The CHANGELOG names eight chord families that the fix covers: `^a`, `[`, `]`, `H`, `W`, `m`, `'`, `y`. The `g` family is the deliberate exception. Top-level user bindings (where the resolver has no pending state) remain unaffected — user keymap consultation runs first at the top level, only the second-key-of-a-chord path changes.

**The rule, named at one if-statement.**

The pre-fix `feed` body opened with an unconditional user-keymap consultation:

```
// User bindings always win. We still reset any pending multi-key
// state so `g` followed by a user-bound key doesn't trigger `gg`.
if let Some(action) = user.find(&ev) {
    self.reset();
    return ResolverOutcome::User(action.clone());
}
```

The post-fix body gates that consultation on `chord_locked`:

```
let chord_locked = !matches!(self.pending, PendingSeq::Normal | PendingSeq::G);
if !chord_locked {
    if let Some(action) = user.find(&ev) {
        self.reset();
        return ResolverOutcome::User(action.clone());
    }
}
```

The whole rule is one boolean and one match: chord-locked iff the pending state is anything other than `Normal` (top level) or `G` (the bare-`g` exception). When chord-locked, the user-keymap consultation is skipped and the rest of `feed` resolves the chord.

The `g`-as-exception rationale lives in the comment: "`g` is the deliberate exception: bare `g` is also a vi motion fragment that users may want to remap (the `user_binding_resets_pending` test covers this), so chords built on `g` (`gd`, `gf`, …) remain user-overridable." The pre-existing `user_binding_resets_pending` test (visible in the file but not part of this PR's diff) covers the `g`-then-user-binding case explicitly; PR #32 adds a `g_chord_remains_user_overridable` test as a counter-test to the new rule.

**The eight chord families, by their `PendingSeq` variants.**

The pre-existing `PendingSeq` enum in `src/keymap/resolver.rs` is:

```
enum PendingSeq {
    Normal,    // top level — no chord in flight
    G,         // bare `g` — the deliberate exception
    Bracket,   // `[` and `]`
    W,         // `^a` chord prefix family
    Mark,      // `m` (set mark) chord prefix
    GotoMark,  // `'` (jump to mark) chord prefix
    Worktree,  // `W` chord prefix
    Yank,      // `y` chord prefix
    Harpoon,   // `H` chord prefix (added by PR #8 — see below)
}
```

PR #32's rule: `chord_locked = !matches!(self.pending, PendingSeq::Normal | PendingSeq::G)`. That covers seven variants — `Bracket`, `W`, `Mark`, `GotoMark`, `Worktree`, `Yank`, `Harpoon` — plus any future variant that doesn't explicitly opt out. The CHANGELOG's "eight chord prefixes" includes both `[` and `]` under `Bracket`; the diff treats them as one family because they share the resolver state.

**The new tests, one per family.**

PR #32 adds seven new test cases for the chord-precedence rule plus one counter-test for the `g`-exception:

- `user_binding_for_n_does_not_preempt_ctrl_a_n`: `^a` then `n`, with `n` user-bound → `Action::PaneNextTab` (chord wins).
- `user_binding_for_p_does_not_preempt_ctrl_a_p`: `^a` then `p`, with `p` user-bound → `Action::PanePrevTab` (chord wins).
- `user_binding_for_g_does_not_preempt_bracket_g`: `]` then `g`, with `g` user-bound → `Action::JumpNextGitChange` (chord wins).
- `user_binding_for_y_second_key_does_not_preempt_yank_chord`: `y` then `p`, with `p` user-bound → `Action::YankPrompt` (chord wins).
- `user_binding_for_digit_does_not_preempt_harpoon_chord`: `H` then `1`, with `1` user-bound → `Action::HarpoonJump(1)` (chord wins).
- `user_binding_for_letter_does_not_preempt_mark_chord`: `m` then `a`, with `a` user-bound → `Action::SetMark('a')` (chord wins).
- `user_binding_for_letter_does_not_preempt_worktree_chord`: `W` then `l`, with `l` user-bound → `Action::WorktreeList` (chord wins).
- `g_chord_remains_user_overridable`: `g` then `d`, with `d` user-bound → `ResolverOutcome::User(BoundAction::UnixCmd("custom-d"))` (user wins; the deliberate exception).

Each chord family gets its own regression test. The `'` (GotoMark) family doesn't get a new test in this diff but is covered by the same `chord_locked` condition (`PendingSeq::GotoMark` is not in the exception list).

**Connection to PR #8 — phase α set up the chord PR #32 corrects.**

The `Harpoon` variant of `PendingSeq` was added by PR #8 (`feat/harpoon`, 62fc129, 2026-05-02; per arc 06's PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W). The `H1`..`H9` chord family is among the broken cases PR #32's CHANGELOG names verbatim. The `user_binding_for_digit_does_not_preempt_harpoon_chord` test in this PR's diff exercises the harpoon family explicitly.

The reading the diffs support: the dispatch precedence bug was latent — the pre-fix `feed` consulted user bindings unconditionally, so any chord family with a user-bound second key would have been preempted. The bug existed before PR #8's `H` family was added; the `^a` and `]`/`[` and `y` and `m`/`'` and `W` families were already present and would have been preempted by user bindings on the same letters. The user report that triggered PR #32 names `^a-n` / `^a-p` specifically — `^a` is one of the older families. So PR #32 is a fix for a latent bug that *would* have surfaced eventually with any chord family the user happened to user-bind a letter into.

What PR #8 contributes: a new chord family — `H` with second keys `1`..`9` — adds new combinations a user could user-bind into. The `user_binding_for_digit_does_not_preempt_harpoon_chord` test makes this explicit. Whether PR #8's introduction of the `H` family is what *prompted* the user report, or whether the report was triggered by `^a-n`/`^a-p` independently, is determinable only from outside the diffs. The commit message names `^a-n`/`^a-p` as the report; PR #8 adds the `H` family to the same precedence issue without being the trigger.

The framing entry's "phase α set up phase β" hedge holds against the diff: the chord-precedence rule applies to all eight families, of which seven (`^a`, `[`, `]`, `W`, `m`, `'`, `y`) predate phase α. Phase α adds one more (`H`) to the list of families the fix protects. The connection is real but not exclusively causal — phase β fixes a bug that existed before phase α and would have surfaced eventually regardless. Phase α widened the surface PR #32 protects.

**The single-rule shape.**

PR #32 is the simplest entry in arc 06 by diff structure: one boolean expression, one if-statement gate, eight test cases. No new types, no new fields, no new files. The whole behavioral change fits in three lines of code (the `chord_locked` let, the `if !chord_locked` wrapper, and the comment). Compared to PR #25's two-cases-plus-diagnostic bundle or PR #8's 1,037-line feature drop, PR #32 is what a single-rule dispatch fix looks like at minimum diff size.

**Drift findings flagged for the insight layer**:

- The `g`-as-exception decision lives in a comment, not a separate enum variant. A future maintainer reading `PendingSeq` won't see "G is the user-overridable family" without reading the `feed` body. The seam is named in the doc-comment; whether it gets refactored into something type-level (e.g. a `bool user_overridable` per variant or an explicit `chord_locked()` method on `PendingSeq`) is the kind of seam arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) framed for `pane_focused`'s three meanings: a working flat-condition shape that holds until policy needs more axes than booleans support.
- The chord-precedence rule reaches every chord family in spyc — present and future — through one if-statement. The reach-through-one-statement shape is structural infrastructure: any new `PendingSeq` variant inherits the rule for free unless the maintainer explicitly adds it to the exception match-arm. Captured for the eventual insight layer's structural-reach catalogue if relevant.
- The bug was latent across at least seven chord families before PR #32, and the user report that surfaced it named one specific family (`^a` with `n`/`p`). Whether the report would have come anyway absent PR #8's new `H` family is a counterfactual the diffs can't answer; the PR-pair shape (phase α adds family, phase β fixes precedence) is what the data points show.

Provenance:
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06).
- `git diff a7867fb^1..a7867fb^2 -- CHANGELOG.md`: `### Fixed` block quoted verbatim above.
- `git diff a7867fb^1..a7867fb^2 -- src/keymap/resolver.rs`: `chord_locked` let-expression and `if !chord_locked` gate quoted in full above; `g`-exception comment quoted above; eight new test cases (seven chord-family tests plus the `g_chord_remains_user_overridable` counter-test).
- `Cargo.toml:3` post-merge: `version = "1.41.19"`.
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (flat-condition-shape-holds-until-policy-needs-more-axes precedent; relevant for the `g`-as-exception comment-vs-type observation).
- `history-arc-06-input-and-overlays` framing entry = 01KR2G8042HWE419X0ESWKN205.
- `history-arc-06-input-and-overlays` PR #8 entry = 01KR2GCH3Q8DR9DATBBC802Q8W (`PendingSeq::Harpoon` variant origin; phase α's new chord family).
- `history-arc-06-input-and-overlays` PR #10 entry = 01KR2GH1D9QCGDPZEMWW09R898 (sibling phase-α entry; quick-select rides the `^a` family that PR #32's rule protects, but doesn't itself add a `PendingSeq` variant).
- `history-arc-06-input-and-overlays` PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (sibling phase-β entry; PR #25's `--key-trace` is the diagnostic infrastructure that would catch the next dispatch-precedence question if one arose).

<!-- Entry-ID: 01KR2GQJSTRYZFKE58F395CEN2 -->

---
Entry: Claude Code (caleb) 2026-05-08T00:46:42.814621+00:00
Role: scribe
Type: Note
Title: Closure: arc 06 baseline complete; cadence-A-at-4-PRs results; arc 07 (codex-and-mcp-bridge) follows

Spec: scribe

tags: #history #arc-06

Arc 06 baseline written. Six head entries: framing → 4 per-PR entries → this closure. Tail entries follow.

**Per-PR entries written, in arc order:**
- Framing = 01KR2G8042HWE419X0ESWKN205 (cadence option A; 2+2 phase split named; capability-accretion-with-corrective-second-half register).
- PR #8 (feat/harpoon, 62fc129, 2026-05-02) = 01KR2GCH3Q8DR9DATBBC802Q8W. Phase α. PARALLEL PATTERN with catalogue §4.
- PR #10 (feat/quickselect, 9043547, 2026-05-02) = 01KR2GH1D9QCGDPZEMWW09R898. Phase α. PARALLEL PATTERN with catalogue §4. §1 numbered-panels: refuted (alphabetic labels, not numeric).
- PR #25 (fix/input-dispatch-hardening, bfc4a18, 2026-05-06) = 01KR2GMSNX29CWFN154QBK6TJ3. Phase β. Two enumerated cases under one symptom + a `--key-trace` diagnostic for the next report.
- PR #32 (fix/chord-priority-over-user-keymap, a7867fb, 2026-05-06) = 01KR2GQJSTRYZFKE58F395CEN2. Phase β. Single-rule fix; eight chord families covered; the `g` chord stays the deliberate exception.

**Cadence-A-at-4-PRs result.**

Arc 05 at 8 PRs needed option A' (per-PR plus phase-grouping in framing) because the closure couldn't carry summarization for 8 PRs. Arc 06 at 4 PRs sat below that threshold; option A with light phase-language in the framing read cleanly. The 2+2 calendar split made the phase shape self-evident — both phase-α PRs landed on 2026-05-02, both phase-β PRs on 2026-05-06 — so the framing didn't need to do heavy structural work to surface it. The verdict for future arcs at this PR count: plain A is sufficient when the calendar gives you the phase split for free; A' is the right refinement only when the PR count alone makes closure-summarization break down.

**Catalogue §4 reading after arc 06.**

Arc 06 adds two PRs (PR #8, PR #10) to the cumulative four-PR §4-alignment family that arc 05's closure (= 01KR2AJVZA1E85YSKHF4FNRQQ3) and story-tail (= 01KR2ANRAEFWWR5W9FQP11A0DB) named. After arc 06, the count holds: four PRs across two arcs hold §4 alignment, zero execute the `PagerView::picker_items: Vec<(Label, Action)>` shape. PR #33 and PR #35 (arc 05) hold DIRECTION ALIGNMENT from the pager-as-mode side; PR #8 and PR #10 (arc 06) hold PARALLEL PATTERN from the standalone-overlay side.

The framing arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) named the catalogue §4 disposition for arc 06's PRs as "parallel-but-different." Arc 06's per-PR entries reconfirm that against the diffs: PR #8's `HarpoonMenu` is a standalone modal `Block` overlay with closed bindings, not a `PagerView` extension; PR #10's `QuickSelect` lives in `src/pane/quick_select.rs` with case-as-intent dispatch over an alphabetic label space, not Enter-to-fire over a generic `Action` substrate. Both honor the catalogue's *intent* — surface picker-shaped UX for fast surfacing — without instantiating §4's specific structural shape.

The deferral question continues: whether the four parallel-and-direction-aligned PRs make `PagerView::picker_items` unnecessary, or whether the picker pattern is structurally still ahead. Per arc 05's story-tail, that question goes to the insight layer; arc 06 does not resolve it.

**The phase-α-to-phase-β connection, reconfirmed against the PR #8 ↔ PR #32 diffs.**

PR #8 introduced the `H` chord prefix family (new `PendingSeq::Harpoon` variant). PR #32 fixed a chord-precedence bug whose CHANGELOG names `H1`..`H9` explicitly among the broken cases. Phase α widened the surface phase β protects.

The connection is real but not exclusively causal. The chord-precedence bug was latent across at least seven chord families before PR #8 (`^a`, `[`, `]`, `W`, `m`, `'`, `y`); the user report that surfaced the bug names `^a-n`/`^a-p` specifically, not the new `H` family. PR #32 fixes a latent bug; PR #8 adds one more chord family to the seven-deep list of families the fix protects. Whether PR #8's introduction of `H` is what prompted the user report or whether the report would have come anyway is a counterfactual the diffs cannot answer. The reading the diffs *do* support: phase α widened the chord-prefix tree, phase β fixed how the resolver treats the second key of any chord — and the work landed in dependency order, not just temporal order.

**Drift findings handed forward to the insight layer:**
- Bundle-pattern recurrence within and across arcs (PR #10's `### Fixed` half under a `feat/` slug; PR #25's hardening + diagnostic; arc 04's PR #15 precedent). Captured at the per-PR entries.
- The `--key-trace` infrastructure ships with no immediate consumer beyond PR #25's defensive fix. Future-proofing-without-a-current-need shape; whether it gets used in a future report is determinable only outside the 22-day window.
- The chord-precedence rule's structural reach: one if-statement gates eight chord families (and any future variant that doesn't opt out). Reach-through-one-statement shape; PR #32 entry flagged.
- The `g`-as-exception decision lives in a comment, not a type-level structure. Same shape arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) named for `pane_focused`'s three meanings.
- PR #10's "labels go stale on scroll" close-on-pane-growth design choice has alternatives (re-scan, freeze, scroll-lock) the eventual insight layer can revisit if recurrence shows up.
- The hash-keyed harpoon persistence (`<basename>.<hash>.toml` with `DefaultHasher`) trades portability for collision avoidance with a known caveat (not Rust-version-stable). Captured at PR #8 entry.

**Next session: arc 07 — `codex-and-mcp-bridge`.** Member PRs per `history-overview` segmentation entry (= 01KR0TWHTC1MPK4KJ08Y9SPE6P): PR #18 (`chore/agents-md-and-mcp-hygiene`), PR #19 (`feat/codex-resume`), PR #21 (`feat/codex-mcp-config`), PR #37 (`fix/mcp-socket-project-scoped-discovery`). Date span: 2026-05-05 to 2026-05-07. The arc executes against the load-bearing thesis at `ROADMAP.md:3-23`; arc 07's PR #37 is the most architecturally consequential and is already cited by `onboarding-architecture` (entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ) as the "recently-strengthened invariant (v1.41.24)."

After arc 07, only arc 08 (`recoverability-and-deps`) remains; Phase 3 (insight layer) blocks on all eight arc threads existing.

This thread (`history-arc-06-input-and-overlays`) remains OPEN for cross-arc references and the eventual Phase 3 insight layer.

Provenance:
- 62fc129 (PR #8 feat/harpoon, 2026-05-02).
- 9043547 (PR #10 feat/quickselect, 2026-05-02).
- bfc4a18 (PR #25 fix/input-dispatch-hardening, 2026-05-06).
- a7867fb (PR #32 fix/chord-priority-over-user-keymap, 2026-05-06).
- All prior thread entries: 01KR2G8042HWE419X0ESWKN205 (framing), 01KR2GCH3Q8DR9DATBBC802Q8W (PR #8), 01KR2GH1D9QCGDPZEMWW09R898 (PR #10), 01KR2GMSNX29CWFN154QBK6TJ3 (PR #25), 01KR2GQJSTRYZFKE58F395CEN2 (PR #32).
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 07 member-PR list).
- `history-overview` PR #5 special-handling entry = 01KR0TYF5F11DA8P5HNPA20DBK (back-reference contract honored at arc 06's PR #8 / PR #10 entries).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; back-reference hub).
- `history-arc-05-pager-surface` closure = 01KR2AJVZA1E85YSKHF4FNRQQ3 (cumulative §4 reading after arc 05; arc 06 carries it forward without resolving).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (PR #37 already cited there as recently-strengthened invariant).
- `onboarding-overview` entry 0 = 01KR0NZNJ3KM6BJY09Q4P9D0NE (front door).

<!-- Entry-ID: 01KR2GT1P722663B8ZK06C50WQ -->

---
Entry: Claude Code (caleb) 2026-05-08T00:49:16.422181+00:00
Role: scribe
Type: Note
Title: Arc 06 told as a story: two questions four days apart, and what makes harpoon and quickselect picker-shaped without being §4-shaped

Spec: scribe

tags: #history #arc-06 #tail

The four PRs span four calendar days. Two new picker-shaped overlays land on 2026-05-02; two dispatch-correctness fixes land on 2026-05-06. Reading them in order, what changes between PR #8 and PR #32 is the question being asked — phase α asks "what new ways can a user summon things by keystroke?" and phase β asks "how should a keystroke reach the handler that consumes it?". Two questions, four answers.

The phase-α answers are the visible work. PR #8 ships harpoon — nine slots, `H1`..`H9` direct dispatch, an `Hh` modal overlay for reordering and deletion, an `=h` listing filter that surfaces ancestor directories so harpooned files inside `src/foo/bar/` keep `foo/` visible while you browse `src/`. PR #10 ships quick select — alphabetic labels over the visible pane content with case-as-intent dispatch (lowercase yank, uppercase open), a 23-letter alphabet that excludes `q`/`Q` (exit) and `j`/`k` (vi-reflex), one or two letters per label depending on match count. Both ship as standalone overlays carrying their own picker structures — `HarpoonMenu` and `QuickSelect` — neither extending `PagerView`. Arc 02's investigation entry catalogued the "Generalized pager picker" as the highest-leverage of the lazygit borrows, with a specific shape: a `PagerView::picker_items: Vec<(Label, Action)>` field with Enter-to-fire dispatch. After arc 06 that field still doesn't exist. What does exist is two parallel-shaped pickers — different surface (overlay rather than pager mode), different dispatch (direct chord and case-as-intent rather than Enter-to-fire over a generalized substrate), different action set (closed and feature-specific rather than parametric).

Arc 02 saw this from the catalogue side. Arc 05 saw it from the cross-arc-direction side: PR #33's visual-line-mode and PR #35's `D`-launches-pager held DIRECTION ALIGNMENT with §4 — the pager-as-mode and pager-as-launchable shapes — without instantiating §4's specific picker pattern. Arc 06 sees it from the standalone-overlay side: PR #8 and PR #10 hold PARALLEL PATTERN — the picker shape, but lifted out of the pager and dropped into `App` and into the pane respectively. Four PRs across two arcs hold §4 alignment. Zero execute the field. What that means for whether `picker_items` is the still-ahead pattern or the unnecessary one is the question arc 05's story-tail returned upstream and arc 06 inherits — narratable from neither arc's diffs alone, fuel for the eventual insight layer.

The catalogue §1 question — "Numbered panels & direct-jump" — almost looks like it should apply to PR #10's labeled overlay, and the framing flagged it. The labels aren't numbers, though. The alphabet is `abcdefghilmnoprstuvwxyz`, three letters omitted on purpose: `q` so you can still exit, `j`/`k` so the user's vi-reflex doesn't fire actions. §1's pattern is direct-jump to a small set of *named top-level surfaces* via numeric chords, ranked **skip** for spyc on the grounds that a two-surface layout doesn't earn the binding cost. PR #10 is something else — ephemeral per-scan label assignments over a content stream. The §1 SKIP recommendation is unaffected; PR #10 is not a §1 instance.

The phase-β answers are the less visible work. PR #25's defensive bundle is two enumerated cases under one symptom — the user reported "switching panes input doesn't work when done too quickly," the report didn't reproduce, and the diff addresses two plausible failure modes (a post-chord bounce on the focus-switch chord, and bracketed-paste sequences silently dropping when there's no prompt and no pane open) bundled with `--key-trace` as the diagnostic for the next time someone files the same report. The diagnostic has no current consumer beyond PR #25's own defensive fix; the infrastructure ships in advance of the next report rather than after one. PR #32's fix is one boolean and one if-statement: when a chord prefix is pending, the next key resolves the chord instead of being preempted by user keybindings — eight chord families covered, the `g` chord stays the deliberate exception because bare `g` is a vi motion fragment users may want to remap. One latent bug, one rule, eight regression tests.

The diff-supported observation between phase α and phase β is small but worth naming. PR #8 introduces a new chord prefix family (the `H` family), and PR #32's fix names `H1`..`H9` explicitly among the broken cases. Phase α widened the chord-prefix tree; phase β corrected how the resolver treats the second key of any chord. The bug PR #32 fixes was latent across at least seven chord families before phase α (`^a`, `[`, `]`, `W`, `m`, `'`, `y` were all present and would have been preempted by user-bindings on the same letters), and the user report names `^a-n`/`^a-p` specifically — not the new family. So phase α didn't *cause* phase β. But phase α landed first, the new chord family joined the list of families the eventual fix would have to protect, and the work landed in dependency order: surface widens, then resolver gets stricter. That sequence reads cleanly even though the user report's specific trigger predates phase α.

The shapes inside each PR's bigger box are worth a beat each. Harpoon is four primitives, not one — direct-jump chord, modal overlay, append/remove pair, listing filter — sharing one `Harpoon` model, an `ancestor_cache` rebuilt on every mutation, and a hash-keyed persistence file (`<basename>.<hash>.toml`) that auto-saves and survives `PROJECT_HOME` swaps via a `reconcile_harpoon` post-hook on the action dispatcher. Quick select is three sub-decisions — what alphabet to use, how to dispatch case-as-intent, what kinds of matches to surface — each of them defended in source comments against the alternatives, plus a `pickable_text` helper on `Pane` that rerouted `gf`/`gF` to honor scroll mode for free. The hardening bundle is two cases plus a diagnostic that ships *with* the defensive fix because the defensive fix admits it doesn't know which case it's actually fixing. The chord-precedence rule is one if-statement, one comment naming `g` as the exception, eight regression tests — the rest of the diff size is regression coverage.

What carries through the whole arc is the relationship between *new surface* and *correct dispatch*. The two phase-α PRs add capability surface: harpoon's `H` chord family, quick-select's `^a u` extension to the existing `^a` family, both with their own modal/picker behaviors that intercept keys before normal dispatch. The two phase-β PRs ensure dispatch correctness as the surface grows: the focus-switch chord doesn't leak the chord-completing key into the now-focused pane child, the resolver doesn't let user-bindings preempt chords mid-flight. The diff order doesn't enforce the connection — phase α's overlays don't *create* phase β's bugs — but the register does: capability accretes, then correctness gets corrected. Arcs 04 and 05 register as capability-accretion arcs without a corrective second half; arc 06 has the second half because the dispatch surface widened enough that what *should* happen when keys arrive became its own concern, separate from what *can* happen.

The arc ships at v1.41.19 — six minor cuts since PR #8's v1.39.0 (one per arc-α PR; four 1.41.x patches in between for unrelated work landing in arcs 03/05/08; PR #25 at v1.41.12, PR #32 at v1.41.19). The pace of small versioned cuts is the rhythm arc 01's reflection tail (= 01KR0XR504ZR10Y242JERT4K9S; restated at arc 01's later tail) named as the v1.41.x cadence. Arc 06 inherits that rhythm without modifying it — none of the four PRs cuts a major or minor version after PR #10's v1.40.0; everything else is a patch.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-06-input-and-overlays` head entries 0–5 = 01KR2G8042HWE419X0ESWKN205 (framing), 01KR2GCH3Q8DR9DATBBC802Q8W (PR #8), 01KR2GH1D9QCGDPZEMWW09R898 (PR #10), 01KR2GMSNX29CWFN154QBK6TJ3 (PR #25), 01KR2GQJSTRYZFKE58F395CEN2 (PR #32), 01KR2GT1P722663B8ZK06C50WQ (closure).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (catalogue §4 source; "parallel-but-different" disposition for PR #8 / PR #10).
- `history-arc-05-pager-surface` PR #33 entry = 01KR2AAX12XSNRNZPTXJT2TXJA (DIRECTION ALIGNMENT precedent — pager-as-mode side of §4).
- `history-arc-05-pager-surface` PR #35 entry = 01KR2AD5PV989H58E49E5D18NM (DIRECTION ALIGNMENT precedent — pager-launchable-from-listing side of §4).
- `history-arc-05-pager-surface` story-tail = 01KR2ANRAEFWWR5W9FQP11A0DB (deferred §4 question to insight layer; this tail carries the question forward without resolving it).
- `history-arc-04-git-integration` story-tail (capability-accretion register precedent without corrective second half).
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (recurring-concern shape precedent; arc 06's capability-and-correction shape is different).
- `notes/lazygit-ux-catalogue.md` §1 (numbered-panels SKIP recommendation; refuted as not-applicable to PR #10's alphabetic labels).

<!-- Entry-ID: 01KR2GYQPQRX08SV980SPHHZ80 -->

---
Entry: Claude Code (caleb) 2026-05-08T00:50:07.140552+00:00
Role: scribe
Type: Note
Title: Two seams the story-tail walks past: --key-trace shipping ahead of a consumer, and the chord-precedence rule's flat exception

Spec: scribe

tags: #history #arc-06 #tail

A pair of specific seams worth pulling out separate from the story-tail above, because they're the kind of thing a reader hitting the head entries can verify in five minutes and easily miss in the broader narrative.

PR #25's `--key-trace` infrastructure ships ahead of any consumer. The diagnostic doesn't fire from any test, doesn't surface in any UX, doesn't get pulled into any analytics. It exists for a future bug report — a user types "input doesn't work when done too quickly," flips the flag, reproduces, and ships the log. The infrastructure shape is *ship the diagnostic with the defensive fix so the next report is reproducible*, and that shape is forward-pointing in a way the ~205-line PR doesn't surface. The `--key-trace` CLI flag, the `SPYC_KEY_TRACE` env-var equivalence, the new `src/key_trace.rs` module's `init`/`is_enabled`/`log` API, the four trace points in `handle_key` and the resolver and the paste path and the bounce-suppression swallow — none of them have a current consumer beyond the immediate one (PR #25's own defensive bundle). Whether the next "input doesn't work" report ever comes, and whether `--key-trace` is what surfaces its root cause, is determinable only outside the 22-day window. The seam to watch: if a future input bug surfaces and ships its cause via a `key-trace` log, the bundle pattern (defensive fix + diagnostic for next time) shifts from speculative to validated. If three such bugs come and `--key-trace` plays no role, the pattern is the kind of forward-proofing that costs upkeep without paying for itself. PR #25 doesn't bet on either reading; the seam is where the bet plays out.

The second seam is smaller and almost the inverse. PR #32's chord-precedence rule lives at one if-statement: `let chord_locked = !matches!(self.pending, PendingSeq::Normal | PendingSeq::G); if !chord_locked { /* consult user keymap */ }`. Eight chord families inherit the rule from one boolean expression. Any new `PendingSeq` variant added by a future PR inherits the rule for free unless its author explicitly extends the exception match-arm to include it. That reach-through-one-statement structural shape is what makes the rule cheap to add and powerful to maintain — but it carries the same kind of seam arc 03's seams-aside (= 01KR11TME2KF5QFQ45GJYG8MC7) named for `pane_focused`'s three meanings and the cursor-block guard's flat conjunction: working flat-condition logic that holds until policy needs more axes than the boolean supports. The `g`-as-exception decision lives in a comment, not a type — `PendingSeq::G` doesn't carry any field saying "user-overridable"; the `feed` body's match-arm is the only place the policy is named. If a second exception ever lands ("the `H` chord should be user-overridable when the user has bound `H1` to something specific"), the maintainer choosing between extending the match-arm and refactoring `PendingSeq` to carry a `chord_locked: bool` per variant is the moment the seam opens. Until then the flat condition is the right shape, and the seam is just a note for whoever lands next on this surface.

Both observations are forward-pointing in the way the story-tail isn't, but neither is a prediction. They're notes about where the next dispatch-related question is most likely to land, given what the diffs in arc 06 actually shaped — small gifts to whoever lands next on this surface.

Provenance:
- No new commit references; this entry reflects on the head entries which carry full SHA provenance.
- `history-arc-06-input-and-overlays` PR #25 entry = 01KR2GMSNX29CWFN154QBK6TJ3 (full `src/key_trace.rs` API and trace-point listing).
- `history-arc-06-input-and-overlays` PR #32 entry = 01KR2GQJSTRYZFKE58F395CEN2 (the `chord_locked` let-expression and `g`-as-exception comment quoted verbatim there).
- `history-arc-06-input-and-overlays` story-tail above = 01KR2GYQPQRX08SV980SPHHZ80.
- `history-arc-03-pane-behavior` seams-aside = 01KR11TME2KF5QFQ45GJYG8MC7 (precedent for the flat-condition-shape-holds-until-policy-needs-more-axes seam observation; this entry's second seam echoes that one across a different surface).

<!-- Entry-ID: 01KR2H094DPBVASNG884XXFEJH -->
