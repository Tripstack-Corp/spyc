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
