# Reviewer E — documentation contract and comment standards

**Subject:** `git diff v2.0.0..HEAD` (HEAD `f8dedae`, 162 commits, version `2.1.0-CURRENT`).
**Scope:** comment house-style drift, `SPYC-TRAP` anchor integrity, FEATURES.md /
KEYBINDINGS.md / HARNESS.md / 2.1-release-notes.md / ABOUT.md against HEAD behavior,
CHANGELOG provenance. Read-only; no repo changes.

---

## Verdict

**The owner's worry about comment slop is not borne out — the code comments are the
strongest part of this diff.** A mechanical scan of all 7,467 added comment-bearing
lines against twelve banned shapes ("for now", "TODO", "until X lands", "with the Y
PR", reasoning leakage, …) produced **zero** true positives, and 70 hand-read hunks
stratified across four author-agent eras produced **two** defects, both stale
*placement* rather than slop content, both from the same day (the 2026-08-05 mouse
campaign). Comment density is high and rising in the feature work (17.7% → 34.8% →
27.2% → 19.7% by era), which is a legitimate topic for issue #31 but is not
drift from the current standard.

**The documentation contract is weaker, but not eroding at the process level.**
AGENTS.md's "Keep docs in sync (same commit, not a follow-up)" is honored in **29 of
32** `feat` commits since v2.0.0; three miss it (#279, #281, #287), and those three are
exactly the individual doc defects filed below — there is no separate process problem
behind them. *(An earlier draft of this report claimed 22 of 32. That was a broken
scan, not a finding; see F-E5 for the retraction and the bug.)* Where the contract
does fail it fails badly: FEATURES.md now states the *opposite* of HEAD on agy's
blocked dot, and its skill section describes two hosts for a three-host feature
because the commit that added the third updated FEATURES.md's agent section and not
its skill section. `docs/drafts/2.1-release-notes.md` is the most serious single
item: it is still verified against a commit 47 behind HEAD and presents the release as
"Two big things", omitting the entire archive-browsing campaign (~20 PRs, a whole new
`src/archive/` module) — the release's largest feature. SPYC-TRAP integrity is clean
(8/8 anchors resolve, guard passes and I re-derived both sides by hand); CHANGELOG is
untouched by hand and byte-reproducible from git-cliff, with one stale header.

---

## Comment audit — method and counts

### Sampling method

Four eras, cut by date/commit range rather than author (all commits are the same human
committer; the eras track the agent sessions behind them):

| era | range | dates | commits | added `src/**.rs` lines | comment density |
|---|---|---|---|---|---|
| **E1** release / skill / agy | `v2.0.0..a783d4b` | Jul 9 – Aug 3 | 23 | 2,018 | 17.7% |
| **E2** mouse campaign | `a783d4b..7b3a551` | Aug 4 – 5 | 31 | 5,250 | 34.8% |
| **E3** audit remediation (C1–C9 / F1–F7) + mouse decomposition | `7b3a551..2223c09` | Aug 5 – 8 | 43 | 7,409 | 27.2% |
| **E4** archive campaign + release batch | `2223c09..HEAD` | Aug 8 – 10 | 65 | 20,219 | 19.7% |

Four passes:

1. **Mechanical, whole diff.** Extracted every added line carrying `//` from
   `git diff v2.0.0..HEAD -- 'src/*.rs'` — **7,467 comment-bearing added lines** —
   and matched twelve banned-shape regexes (`for now`; `TODO|FIXME|HACK|XXX`;
   `until … lands/ships`; `will be added/removed/replaced`; `with the … PR`;
   `we should/could/might`; `follow-up/later PR/future PR`; `not implemented yet`;
   `is planned/plan to`; the full 21-phrase reasoning-leakage list; `temporary
   workaround/for the time being`).
   **Hits: 11. True positives: 0.** Every hit is a legitimate use of the words
   (`until \`target\` lands inside one`, `a follow-up yank can still find it`,
   `no filesystem path we could be wrong about`).
2. **Structured read, 34 hunks.** Largest hunk in each of 34 files, chosen for area
   spread (skill, fs, git, watch, mcp, agent, pane, clipboard, ui/pager, mouse ×4,
   codex_pin, about, line_select, pane_tabs, pager_handler, guard_support, archive ×6,
   image_ops, image_gallery, paste_capture, transcript_images, harpoon, hook_owners),
   6/7/9/12 across E1–E4.
3. **Random read, 36 hunks.** Seeded (`random.seed(21)`) sample of 9 medium hunks
   (8–45 added lines) per era from the full `src/` diff, excluding test-only files —
   deliberately *not* the module headers, where the best writing lives.
4. **Heuristic scan for "restates the code".** Every short (<65 char) added line
   comment whose content words overlap ≥60% with the code line beneath it.
   **2 candidates, 0 true positives** (`// "hi" -> aGk=` above a base64 assertion;
   `// The editor saves.` as a test stage direction).

**Total hunks read: 70. Met the standard: 68. Missed: 2.**

### The two misses (and there is no worse third)

Both are **stale placement**, not slop — a doc block left attached to the wrong item
when its function moved, and a comment whose referent was removed. Both landed on
2026-08-05 in the mouse campaign (E2), the era my prior notes already flag as the
low-quality day.

1. `src/app/render/mod.rs:450–469` — `prepare_frame`'s doc comment was orphaned onto
   `frame_layout` when `6515ac3` (#219) extracted the latter. `frame_layout` now
   carries two stacked doc blocks; the first ("Pre-draw pass: compute the frame layout
   and **settle the derived list state** … The list settle runs only on the file-list
   path") is factually wrong for a `&self` function that settles nothing, and
   `prepare_frame` (line 501) is left undocumented. Rustdoc renders both as one block.
2. `src/lib.rs:941–943` — `// \`suspend_tui\` already cleared this; the reconcile
   re-enables per config…` sits directly above `terminal.hide_cursor()?;`, which it
   does not describe. Introduced dangling by `c8c820e` (#218): "this" has no referent
   in the code beneath it (it means mouse capture, which `resume_tui` deliberately does
   not re-enable). AGENTS.md: "never narrate what the code says" — this narrates
   something the code does not say at all.

### What the sample found on the positive side

The E1/E3/E4 hunks are consistently at the standard the house style asks for: the
comment names the failure that motivated the code, not the code.
`src/app/watch.rs:36–47` (the inotify storm, with the observed 21k events/s and why
macOS hides it), `src/mcp/hooks.rs:187–200` (absent vs. unparseable as a data-loss
distinction), `src/archive/index.rs:1–8` (zip-slip made structurally impossible rather
than guarded), `src/state/transcript_images.rs:14–27` (an explicit "what the format
does NOT give you" section), `src/guard_support.rs:1–25` (the two fail-open shapes,
with file:line for each). Comments were also *maintained*: `src/app/mouse_mode.rs:78–80`
was updated when `[mouse] capture` flipped to default-on ("`capture` now defaults to
true, so this starting condition is set explicitly rather than assumed").

---

## Docs claim vs HEAD

Only rows where the doc and HEAD disagree, or where HEAD has behavior the doc lacks.
Everything else checked out — see "Verified correct".

| doc:line | claim | verdict | evidence |
|---|---|---|---|
| `FEATURES.md:257–261` | agy's hooks "cover `working` + `done` only and the red `blocked` … square comes from spyc reading agy's approval prompt off the pane instead" | **WRONG on HEAD** | `src/mcp/hooks.rs:493–495`: "PARTIAL — `working` + `done`, **plus `blocked` for the `ask_question` tool**". Only the tool-*permission* half is scraped. `6e053f8` (#287) updated `docs/AGENT_ORCHESTRATION.md` and not FEATURES.md |
| `FEATURES.md:1204–1214` | `--install-skill` writes to `~/.claude/skills/spyc/` and `~/.codex/skills/spyc/`; "one embedded copy serves **both**"; "adding another host is one `Host` variant" | **WRONG on HEAD** | three hosts: `src/skill/mod.rs:191` iterates `[Host::Claude, Host::Codex, Host::Agy]`; agy resolves to `~/.gemini/config/skills/spyc` (`:178–186`). Added by `78fe33f` (#194). AGENTS.md is correct |
| `FEATURES.md:1253` | "`:skill remove` — delete `~/.claude/skills/spyc/`" | **WRONG** | removes from every host and reports which: `src/skill/mod.rs:308–318`, `src/app/skill.rs:120–136` |
| `FEATURES.md:52–53` | "a misnamed `.png` full of text still hex-dumps" | **WRONG** | `plan_image_open` declines (`src/app/pager_handler/mod.rs:707–716`), then `build_pager_view` branches on `shell::looks_like_text` (`:817`) and text content takes the **text** pager with syntax highlighting; the hex branch (`:987–994`) needs NUL bytes |
| `FEATURES.md:678, 693` | left-click "focuses that region"; divider documented only as a *selection* surface | **INCOMPLETE** | left-clicking a tab in the divider **switches to it**: `src/app/mouse/tab_hit.rs`, `src/app/mouse/mod.rs:281–283`, `src/app/mouse/route.rs:105`. `309f838` (#279), zero docs |
| `FEATURES.md:693` | chrome drag-select lists "the status line or the divider/tab line" | **INCOMPLETE** | four chrome surfaces on HEAD; the **prompt row** and the **activity HUD** are also selectable (`src/app/render/inner.rs:357,365,425`, `render/overlays.rs:594`). `6fab158` (#281), zero docs |
| FEATURES.md (whole file) | — | **MINOR** | `[mouse] invert_scroll` (`src/config/mod.rs:349–360`) has zero occurrences in FEATURES.md. Documented where a config key belongs — CONFIGURATION.md, by `9e9179d` (#260) — so this is a cross-reference gap, not a missing doc |
| `docs/KEYBINDINGS.md` (whole file) | "The complete keymap. Press `?` inside spyc for the same reference" (`:3`) | **MISSING** | zero occurrences of "mouse", "wheel", "drag" or "click". Mouse capture is **default-on** (`src/config/mod.rs:366`), ~3,900 new lines of `src/app/mouse/`, and `:mouse off` — the escape hatch for a user whose native click-drag selection just stopped working — appears nowhere in the keymap reference |
| `docs/KEYBINDINGS.md` | — | **MISSING** | `:images` (`src/app/command_table.rs:113`) is in neither KEYBINDINGS.md nor `src/ui/help.rs`; `:mouse` (`:118`) and the `:about` spelling (`:98`) are absent from KEYBINDINGS.md |
| `docs/HARNESS.md:126` | agy resume = "`--continue` (resumes the most recent for this cwd)" | **WRONG (incomplete)** | `src/agent/mod.rs:688–693`: `--conversation <uuid>` is the primary path (pinned from agy's hook payload, #202); `--continue` is the fallback. The codex row correctly gives both forms, so agy is the one row that reads as if it were as limited as zot |
| `docs/drafts/2.1-release-notes.md:8` | "Verified against `git log v2.0.3..2eef3a7` — 102 commits. Every item below is merged; there are no pending entries" | **STALE** | count is exact at that sha, but HEAD is 47 commits further on (`v2.0.3..HEAD` = 149) |
| `docs/drafts/2.1-release-notes.md:14` | "Two big things, plus a launch-readiness pass" | **STALE — headline omission** | three: archive browsing (#301–#343, ~20 PRs, `src/archive/` + 3 `src/app/archive*.rs` + a `:archive` family) is entirely absent |
| `docs/drafts/2.1-release-notes.md:41–42` | for non-mouse children spyc sends the agent's scroll key, "escalating a sustained streak to page keys" | **OVERSTATED** | `fast_wheel_scroll` is overridden only by `CodexProfile` (`src/agent/mod.rs:518`); agy takes the no-escalation branch (`src/app/mouse/scroll.rs:141–158`, test at `src/agent/mod.rs:974`). FEATURES.md:705–713 scopes it correctly to codex |
| `docs/drafts/2.1-release-notes.md:213–215` | git-status forever-walk fixed "(#193, #255)" | **INCOMPLETE ATTRIBUTION** | true on HEAD, but the walk was still live after #255 — `repo_status_stable` filled the cache without the config half. Closed by `3d29bdd` (#303), which observed `generation` 3879 on a live column |
| `docs/drafts/2.1-release-notes.md:139` | "`^a c` rc double-source (issue #275)" | **WRONG CITATION** | #275 is the PR that fixed it (`6a9cdbb`); GitHub shares the number space, so it cannot also be the issue |
| `docs/drafts/2.1-release-notes.md:324` | "Errata: none known at time of writing" | **STALE** | the git-status entry above is a known erratum |
| `CHANGELOG.md:3` | "cut a release with `make release-tag VERSION=x.y.z`" | **STALE** | `cliff.toml:17–22` (the generator's own header template, changed by `69a5ff1` #186) now reads "`make release-prep` then `make release-tag` (see `docs/RELEASE_ENGINEERING.md`)". `git-cliff --prepend` never rewrites the header, so the file's front-matter still describes the one-step process #186 replaced |
| `AGENTS.md:64` | the `mouse/` bullet enumerates `route.rs` / `selection.rs` / `scroll.rs` / `forward.rs` / `mod.rs` | **INCOMPLETE** | `src/app/mouse/tab_hit.rs` exists (`309f838` #279) and is absent. The enumeration reads as exhaustive; `every_app_module_is_in_the_agents_index` cannot catch it (it scans `src/app/*.rs` only, skipping every subdirectory) |

---

## Findings, severity-ordered

### F-E1 — high — `docs/drafts/2.1-release-notes.md:8,14` — the tag-time notes omit the release's largest feature

The notes' own banner pins them to `2eef3a7`; HEAD is 47 commits past it, and those 47
commits are the entire archive-browsing campaign — `src/archive/` (index, journal,
listing, budget, read, write, scan, mount), `src/app/archive.rs` / `archive_ops.rs` /
`archive_route.rs` / `state/archive.rs`, a `:archive info|list|write|discard|unmount|
cancel` command family, nested mounts, verified write-back, MCP member reads, session
restore into a mount. Line 14 tells a reader the release is "Two big things".
Also missing from the notes: full-screen image preview (#300), `^a g` image gallery
(#302), pasted-image capture (#304), `^`/`$` search anchors (#296 — merged *before*
the notes' last edit), `^z` on any non-shell tab (#306), the viewer-temp-path security
fix (#316), the sibling-spyc hook-strip fix (#315), and #342, a defect in a feature the
notes advertise as finished.
**A fix must:** re-verify the notes against `v2.0.3..HEAD`, add an archive section as a
third headline, and re-run the omissions sweep. This is the deliverable the tag ships
with, so it holds the tag.

### F-E2 — medium — `FEATURES.md:257–261` — states the opposite of HEAD on agy's blocked dot

FEATURES.md tells the user agy's hooks cannot report `blocked`; `src/mcp/hooks.rs:493`
says they do, for the `ask_question` tool. `6e053f8` (#287) updated
`docs/AGENT_ORCHESTRATION.md` and left FEATURES.md contradicting it, so the two
user-facing docs now disagree with each other.
**A fix must:** rewrite the agy sentence to distinguish the two halves of `blocked`
(`ask_question` → hook; tool approval → scrape), matching `hooks.rs:493–505`.

### F-E3 — medium — `FEATURES.md:1204–1214, 1253` — the skill section is a two-host doc for a three-host feature

The shell block shows two paths, the prose says "one embedded copy serves **both**",
"adding another host is one `Host` variant" reads as future work when agy is already
that host, and `:skill remove` is documented as deleting one directory when it clears
all three. `78fe33f` (#194) added `Host::Agy` *and* updated FEATURES.md — but its
agent/MCP sections only, leaving the skill section ~900 lines below untouched. An agy
user reading FEATURES.md concludes the skill does not apply to them.
**A fix must:** add the `~/.gemini/config/skills/spyc/` line, change "both" to "all
three", and correct the `:skill remove` row.

### F-E4 — medium — `docs/KEYBINDINGS.md` — a default-on input surface is absent from "the complete keymap"

Zero occurrences of mouse/wheel/drag/click in the file that opens "The complete keymap.
Press `?` inside spyc for the same reference as an overlay". `[mouse] capture` defaults
to `true` (`src/config/mod.rs:366`), so every 2.1 user gets wheel routing, four
drag-select surfaces, three button gestures and tab-click without a keymap entry —
and, worse, without `:mouse off`, which is the documented remedy for the native
click-drag selection the feature costs them. `src/ui/help.rs:279–282` carries one line;
FEATURES.md:677–713 carries the full account.
**A fix must:** add a mouse section to KEYBINDINGS.md and the corresponding `?` rows,
at minimum the three buttons, the wheel, the four drag surfaces and `:mouse on|off|auto`.

### F-E5 — RETRACTED — "22 of 32 `feat` commits shipped with no doc change"

**Withdrawn. The number was a scanning bug, and the true magnitude is 2–3, which is
already fully covered by F-E2 and F-E6.** Corrected by the review coordinator; their
count was right and mine was not.

Correct measurement over `v2.0.0..HEAD` (denominator 32, subject-anchored `feat`,
per-commit `git show --name-only --format=''`):

| criterion | count | commits |
|---|---|---|
| touched **no** `.md` and not `src/ui/help.rs` | **2** | `6fab158` (#281), `309f838` (#279) |
| touched no *contract* doc (README/FEATURES/AGENTS/ARCHITECTURE/DESIGN/CONFIGURATION/INSTALL/ROADMAP/KEYBINDINGS/help.rs) | **3** | the two above + `6e053f8` (#287) |

So the rule is honored in **29 of 32**, and the three exceptions are not an independent
process finding — they *are* F-E6 (#279, #281) and F-E2 (#287). Filing them twice would
have double-counted the same two commits and overstated the erosion.

**The bug:** my scan piped `git show --stat` into `grep -cE '\.md \||\.md$|…'`.
`--stat` pads every filename to a common column, so `\.md \|` (one literal space before
the pipe) matched only when the `.md` happened to be the *longest* path in that commit,
and `\.md$` never matches `--stat` output at all because each line ends in the
histogram. `#301` demonstrates it: it touches `AGENTS.md`, which renders as
`AGENTS.md·························|···1·+` and was scored as no-doc. An earlier scan
in the same session used `--stat` with a leading-space anchor and no trailing pipe, and
correctly returned exactly the 3 above — I then trusted the later, broken run over it.
Lesson for the other reviewers: parse `--name-only`, never `--stat`.

Two related gaps survive the correction but are smaller than I framed them, because
both commits *did* update docs — just not every affected one:
- `9e9179d` (#260) documented `[mouse] invert_scroll` in **CONFIGURATION.md**, which is
  the right home for a config key; FEATURES.md's mouse section does not mention it.
  Informational at most.
- `78fe33f` (#194) updated AGENTS.md, FEATURES.md and AGENT_ORCHESTRATION.md when it
  added `Host::Agy`, but updated FEATURES.md's *agent/MCP* sections and not its
  *Installable agent skill* section 900 lines further down. That is the real shape of
  F-E3: a partial in-commit update, not a skipped one.

### F-E6 — medium — `FEATURES.md:678, 693` — two shipped mouse behaviors undocumented, in enumerations that read as complete

`309f838` (#279) made a tab click switch tabs; FEATURES.md still says left-click
"focuses that region" and describes the divider only as a selection surface, so the one
part of that row meaning "go here" is documented as static text — the exact framing the
commit message used for the bug it fixed. `6fab158` (#281) added the prompt row and the
activity HUD as selectable chrome; FEATURES.md:693 lists two of the four surfaces.
Neither is in KEYBINDINGS.md or `src/ui/help.rs`. `AGENTS.md:64` has the same gap
(`tab_hit.rs`).
**A fix must:** add the tab-click gesture and the two chrome surfaces to
FEATURES.md/KEYBINDINGS.md/help.rs, and add `tab_hit.rs` to the AGENTS.md `mouse/`
bullet.

### F-E7 — medium — `src/app/render/mod.rs:450–469` — orphaned doc block documents the wrong function

`prepare_frame`'s doc was left attached to `frame_layout` when `6515ac3` (#219)
extracted it. `frame_layout` is `&self` and settles nothing; its rendered doc opens by
describing a settle pass. `prepare_frame` (`:501`) has no doc.
**A fix must:** move the first block back onto `prepare_frame`.

### F-E8 — low — `docs/HARNESS.md:126` — agy's resume row drops its primary path

The table gives agy `--continue` only. `src/agent/mod.rs:688–693` bakes
`--conversation <uuid>` when a session id is pinned, with `--continue` as the fallback;
test `agy_restore_bakes_conversation_or_continues` (`:1130–1142`) pins both. Because
the zot row explicitly says spyc captures no specific session, the agy row reads as the
same limitation when it is not — in the document whose job is per-agent quirks.
**A fix must:** give the agy row both forms, as the codex row already does.

### F-E9 — low — `CHANGELOG.md:3` — generated header no longer matches its generator

`cliff.toml:17–22` was updated by `69a5ff1` (#186) to the two-step release; the
CHANGELOG's own header still names the one-step `make release-tag VERSION=x.y.z`
(last touched 2026-06-09, `da77546`). `git-cliff --prepend` does not rewrite the
header, so this will not self-heal.
**A fix must:** regenerate or hand-sync the header line at the next release cut. Note
this is *not* a manual edit — it is a generated line that stopped being regenerated.

### F-E10 — low — `FEATURES.md:52–53` — "a misnamed `.png` full of text still hex-dumps"

It opens in the text pager, syntax-highlighted. `plan_image_open` declines a non-image
(`src/app/pager_handler/mod.rs:707–716`) and `build_pager_view` then routes on
`shell::looks_like_text` (`:817`); the hex branch (`:987–994`) requires NUL bytes. The
source comment at `:711` says "rather than a hex dump" about what an *image* would
otherwise get; the doc turned that into a claim about text.
**A fix must:** say the file opens as text.

### F-E11 — low — `docs/drafts/2.1-release-notes.md:41–42, 139, 213–215, 324` — four accuracy defects

Escalation-to-page-keys claimed for all non-mouse children but implemented only for
codex; `#275` cited as an issue when it is the PR; the git-status fix attributed to
`#193, #255` when `#303` is what actually closed it; "Errata: none known" while that
erratum stands.
**A fix must:** scope the escalation sentence to codex, drop the "(issue #275)" tag or
name the real issue, add #303, and move the git-status item to Errata or restate it.

### F-E12 — low — `AGENTS.md:116` attributes two rules to one guard; only one is machine-checked

The bullet reads "No 'for now' / 'until X lands' / 'with the Y PR' — they rot into
lies. … never commit reasoning-in-progress. Guard:
`comments_carry_no_reasoning_leakage`." That guard's `SLOP` list
(`src/app/mod_tests.rs:300–322`) is 21 deliberation phrases and contains **none** of
the three temporal shapes the sentence names. The guard's own doc is honest about this
("A curated, high-signal phrase list — NOT a density cap"); AGENTS.md overclaims. No
damage today — the only three `for now` / `TODO` hits in `src/` all predate v2.0.0
(`inventory_ops.rs:10`, `state/mod.rs:241`, `keymap/action.rs:195`, `config/dsl.rs:59`)
— so this is an assurance gap, not a live violation.
**A fix must:** either extend `SLOP` with the temporal shapes or reword AGENTS.md to
say what the guard covers.

### F-E13 — low — `every_app_module_is_in_the_agents_index` cannot see subdirectory modules

`src/app/mod_tests.rs:131` skips anything without a `.rs` extension, which is every
subdirectory, so `src/app/{render,state,mouse,key_dispatch,pager_handler}/**` is
outside the guard. AGENTS.md's convention ("subdir modules are documented as groups")
makes that mostly correct — but the `mouse/` bullet is an explicit per-file
enumeration, and it silently lost `tab_hit.rs` (F-E6). Only five subdir modules are
unnamed anywhere in AGENTS.md; four of them (`key_dispatch/{confirms,prompts}.rs`,
`pager_handler/{motion,pickers}.rs`) are legitimately covered by their group's role
description. `tab_hit.rs` is the one real miss.
**A fix must:** decide whether an enumerating bullet should be guarded, or drop the
enumeration in favor of a role description like the sibling bullets.

### F-E14 — low — `src/lib.rs:941–943` — comment with no referent

See the comment audit above.
**A fix must:** delete it or attach it to the `Effect::SetMouseMode` reconcile it
describes.

### F-E15 — informational — `^a g` is bound but absent from the which-key continuations

`src/keymap/resolver/mod.rs:326` binds `^a g` → `OpenImageGallery`; the
`PendingSeq::W` continuation list (`:181–206`) lists `u`, `\`, `Space` and the rest but
not `g` (`^a C`, an alias of `^a \`, is likewise absent). FEATURES.md sells chord hints
as "the discovery surface for the dense keymap", so a bound-but-unhinted pane key is a
discoverability hole. Belongs to whoever owns keymap this review; recorded here because
it is the same "shipped without its surface" pattern as F-E6.

### F-E16 — informational — `src/ui/help.rs:58` teaches `Space` as "move right one column"

`("l  →  Space", "move right one column")`. `Space` has been the leader since the chord
overhaul (`resolver/mod.rs:628–631` → `PendingSeq::Leader`; the comment at `:624–627`
says explicitly it "used to be a redundant alias for Right"). No `Char(' ')` → `Right`
path exists. KEYBINDINGS.md:15 is correct. **Predates v2.0.0** (`63b728d`,
2026-04-15), so it is outside the diff subject — reported because `?` is the doc
surface KEYBINDINGS.md claims to mirror.

---

## Premises checked

- **"SPYC-TRAP anchors added or moved since 2.0 still resolve"** — **confirmed, and the
  guard is sound.** Six markers and ten anchor sites are new since v2.0.0. I re-derived
  both sides independently of the test: 8 distinct slugs in `src/`
  (`viewer-temp-symlink`, `fs-watch-readonly-access`, `scrape-scan-ignores-live-report`,
  `cursor-read-ssh`, `iterm-osc1337`, `pane-shell-rc-double-source`,
  `signal-teardown-precomputed`, `git-poll-key-single-source`) and exactly those 8
  markers in ARCHITECTURE.md (75, 98, 115, 218, 262, 366, 387, 432). The ninth,
  `ARCHITECTURE.md:585`, is the prose template `<!-- SPYC-TRAP: <slug> -->` and is
  correctly rejected by `slugs_between`'s `[a-z0-9-]` filter. `cargo test --lib
  guard_tests` — all 7 pass. **Coverage caveat:** `scan_rs` skips `*_tests.rs`,
  `mod_tests.rs`, `test_harness.rs` and `tests/` / `*_tests/` directories, and the
  top-level `tests/` and `fuzz/` trees are outside `src/` entirely, so ARCHITECTURE.md's
  "every `SPYC-TRAP(<slug>)` in `src/`" is marginally broader than what runs. Today no
  anchor lives in a skipped location (only `mod_tests.rs`, the guard's own home, using
  the inert template), so the guard is correct on HEAD.
- **"CHANGELOG untouched by hand"** — **confirmed.** Exactly two hunks since v2.0.0
  (`189b9fd` v2.0.2, `f52378d` v2.0.3), both release-cut prepends. I regenerated both
  with the repo's own config — `git-cliff --config cliff.toml v2.0.0..v2.0.2` and
  `v2.0.2..v2.0.3` — and the section bodies are **byte-identical** to what is committed.
  The only divergence is the file header (F-E9), which is a *missed* regeneration, the
  opposite of a manual edit. There is no `2.0.1` section because there is no `v2.0.1`
  tag — correct, not a gap.
- **"ABOUT.md bundled copy vs docs/ABOUT.md … the two are in sync"** — **the premise is
  wrong: there is only one file.** `src/app/about.rs:11` is
  `include_str!("../../docs/ABOUT.md")`, and `find` over the tree returns exactly one
  `ABOUT.md`. Drift is structurally impossible and cargo rebuilds on edit. The sentinel
  test does exist (`src/app/about.rs:64`, `bundled_copy_keeps_its_load_bearing_lines`)
  but guards a **different and real** risk — an agent regenerating hand-written prose —
  by pinning four sentences and three headings; all seven verified present on HEAD, test
  passes. It cannot catch a substantive rewrite that preserves those seven strings, and
  the doc comment says as much. No finding.
- **"Issue #31 proposes reconsidering the verbose style; measure drift from the current
  standard"** — measured. Density by era: 17.7 / 34.8 / 27.2 / 19.7 %, against
  AGENTS.md's stated ~22% design point. The E2 mouse campaign is the outlier (34.8%),
  and it is also the only era that produced comment defects — but they are staleness,
  not verbosity, so the two are not the same complaint. Where verbosity is visible
  (`src/app/mouse_mode.rs:34–52`: 19 doc lines over a 6-line function) every
  paragraph still carries non-obvious information. **No drift from the current
  standard; the density question is genuinely #31's, not a defect.**
- **"FEATURES.md +304 lines"** — actual is +326 / −41 on HEAD (net +285), file now 1,421
  lines. KEYBINDINGS.md is +90 / −7, file 301 lines. Immaterial to the charter, noted
  for the record.
- **"comments_carry_no_reasoning_leakage" is the guard for AGENTS.md's comment rule** —
  partially. See F-E12: it enforces the reasoning-leakage half and nothing of the
  temporal half.

---

## Verified correct

Spot-checks that came back clean, so they are not re-litigated:

- **Comment style, whole diff.** 7,467 added comment lines, twelve banned-shape
  regexes, zero true positives. No `TODO`/`FIXME`/`HACK`/`XXX` anywhere in added
  comments. No reasoning leakage. No dangling identifier references: every
  backtick-quoted Rust identifier in an added comment resolves somewhere in `src/`
  (0 of ~1,400 distinct). No commented-out code blocks in the sample. One pre-existing
  slop comment was **removed** by this diff (`src/app/harpoon.rs`, the `dd`-arming
  block that debated `piggyback on cursor's high bit would be hacky — keep it simple`).
- **Doc link integrity.** All relative Markdown links across README, AGENTS,
  ARCHITECTURE, FEATURES, DESIGN, CONFIGURATION, CHANGELOG, INSTALL, ROADMAP,
  CONTRIBUTING, SECURITY, KEYBINDINGS, HARNESS, AGENT_ORCHESTRATION, ABOUT,
  RELEASE_ENGINEERING, the 2.1 notes and ARCHIVE_BROWSING_PLAN resolve — **0 broken**.
- **KEYBINDINGS.md's changed hunks, key by key.** Every binding in every changed hunk
  matches the defaults: `^B`/`^F` (`resolver/mod.rs:380–381`), `R` (`:797`),
  `^`/`$` anchors (`app/matcher.rs:82–105`), `^T` (`:382`), `yf`/`ya` (`:526,525`),
  `z` (`:709`), `gr` / `]g` / `[g` (`:452,502–517`), `^a K`/`x`/`^a`/`r`/`R` (`:309,
  268–271,313,314`), `^z` (`key_dispatch/mod.rs:82–92`), `^a i`/`+`/`-`/`↓`/`g`
  (`:324,317–318,330,326`), the seven image-overlay verbs (`pager_handler/image.rs:26–38`),
  `Space s`/`?`/`a` (`:362,363,364`), `:archive`'s exact subcommand set
  (`app/archive.rs:720–751`). **Zero wrong entries.** Both new default bindings since
  v2.0.0 (`^a g`, `Space a`) are documented in both KEYBINDINGS.md and help.rs.
- **FEATURES.md's archive section.** ~30 claims checked against `src/archive/` and
  `src/app/archive*.rs` and all verified, including the exact refusal set
  (`state/archive.rs:23–45`, `archive_route.rs:188,328`, `commands.rs:872–876`), the
  badge format (`journal.rs:77–89`), `extract_budget_mb` = 512 / `snapshot_max_mb` = 64
  / `max_depth` = 2 (`config/mod.rs:487,492,491`), the write-back's
  temp→verify→snapshot→rename order (`archive/write.rs:97–120`), `D` on the archive
  itself still hex-dumping, and the MCP scoping story (`mcp/readers.rs:134–199`).
  Only `[archive] enable` / `warn_over_mb` / `max_entries` / `write_back` go unnamed,
  which is CONFIGURATION.md's job.
- **FEATURES.md's git-gutter section** (`:87–102`): all six glyphs, all position rules
  and the `!!` conflict case match `ui/list_view.rs:446–491` and `git/status.rs:79–83`.
- **HARNESS.md**, two of three spot-checks: `decide_scroll_source` is a pure `const fn`
  at `src/app/pane_scroll.rs:42–50` with the three-row ladder exactly as tabulated; the
  2-second debounced autosave is `AUTOSAVE_DEBOUNCE` at `src/app/session.rs:30`, armed
  at `:56`, scheduled at `scheduler.rs:73`. The §2 screen-mode/wheel table also
  verifies (codex plain arrows + `PageUp`/`PageDown` escalation, agy Shift+Arrow, zot a
  plain child). The §1 claim about Claude Code's `managed-settings.json` path is
  unverifiable from this tree — it is a claim about another program.
- **2.1 release notes**, everything not listed in F-E1/F-E11: ~50 promises checked and
  delivered, including all 15 milestone issues closed, the four `[clipboard]` /
  `[mouse]` config keys, DECCKM arrow encoding (`pane/input.rs:11–12,131,524`),
  `--version` printing the SHA, `L` leading with NAME, `:mouse off` surviving a config
  reload, the SIGTERM/SIGHUP restore, `make check-ci`'s GATE sentinel, and the weekly
  fuzz cron.
