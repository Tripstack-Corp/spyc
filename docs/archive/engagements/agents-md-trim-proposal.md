# AGENTS.md trim — cut list awaiting approval (F7)

**Status:** proposal. No edits made to AGENTS.md.
**Measured at:** `72d3ae7` — 163 lines, 42,855 bytes, loaded into every agent
turn in this repo.

## The finding is real, and one section owns it

| Section | bytes | share | load-bearing markers |
|---|---|---|---|
| **`## What it does`** | **16,270** | **38%** | **0** |
| `## Architecture` | 10,801 | 25% | 1 |
| `## Conventions` | 7,146 | 17% | **17** |
| `## MCP tools` | 3,109 | 7% | 0 |
| `### Commits, merges, CHANGELOG` | 1,757 | 4% | 0 |
| `## MVU invariants` | 1,138 | 3% | 1 |
| everything else | ~2,600 | 6% | 0 |

"Load-bearing markers" counts mentions of a guard name, `SPYC-TRAP`, or the
word *invariant* — the things F7's hard constraint forbids removing.

The distribution answers the question on its own: **the largest section carries
the fewest rules.** `## Conventions` is 17% of the bytes and holds 17 of the 20
markers; `## What it does` is 38% of the bytes and holds none.

## The two bullets

Inside `## What it does`, two bullets are 24% of the *entire file*:

| Bullet | bytes | already documented in |
|---|---|---|
| **Agent-activity dots** | 6,691 | `docs/AGENT_ORCHESTRATION.md` (which the bullet itself links) |
| **Lua scripting** | 3,491 | `docs/archive/LUA_SCRIPTING_PLAN.md`, `src/lua/`, CONFIGURATION.md |

Both end by pointing at a dedicated document that covers the same ground in
more depth. The bullet is a summary that grew into a duplicate.

AGENTS.md's own header already states the intent: *"the full feature reference
in [`FEATURES.md`](FEATURES.md)"* and *"**Keep this file slim** — it's always in
context."* The file stopped honouring its own instruction.

## Proposed cuts

**Target: ~43KB → ~26KB (−40%), removing zero rules.**

### 1. `## What it does` → one line per feature (−11KB)

The section header already promises "One line per feature; see FEATURES.md for
the full reference." Make that true. Concretely:

- **Agent-activity dots** — 6,691 → ~250 bytes. Keep: what the dot shapes and
  colours mean, that status comes from self-report and not screen-scraping, and
  the pointer to `docs/AGENT_ORCHESTRATION.md`. Drop: the P0/P1/P1-2/P1-3
  tier mechanics, per-agent hook wiring, `idle_prompt` downgrade, notification
  channel gating. None of it changes what an agent may or may not do.
- **Lua scripting** — 3,491 → ~250. Keep: `$HOME`-only, off-thread, the
  `map KEY lua` entry point, engine location. Drop: the API enumeration,
  event-tier taxonomy, re-entrancy guard narrative.
- **Installable agent skill** (1,172), **Leader/global menu** (538),
  **`:` command line** (498), **Session save/restore** (481) — halve each,
  keep the pointer.
- The remaining ~17 bullets are already one-liners. Leave them.

**Why safe:** zero load-bearing markers in this section. It describes what spyc
*does*, not what a contributor *must not do*.

### 2. `## Architecture` module index → keep, trim prose (−3KB)

The per-module index is genuinely useful for navigation and is **guard-checked**
(`every_app_module_is_in_the_agents_index`). Keep every module line.

Trim only the multi-sentence explanations attached to some entries where
ARCHITECTURE.md already carries the reasoning. Target the entry text, never the
entry itself — deleting a module line breaks the guard, which is the correct
outcome and a useful backstop.

### 3. `## MCP tools` → tighten (−1.5KB)

Overlaps the MCP server's own `SERVER_INSTRUCTIONS`, which agents already
receive at `initialize`. Keep the tool list and the "prefer these over shell
equivalents" framing; drop the per-tool parameter detail that the tool schemas
already carry.

### 4. Leave entirely alone

- **`## Conventions`** (17 of 20 markers) — every rule here is exactly what the
  hard constraint protects. Not one byte.
- **`## MVU invariants (don't erode)`** — the name says it.
- **`### Commits, merges, CHANGELOG`** — the conventional-commit rule is
  load-bearing: `filter_unconventional` silently drops an untyped subject from
  the CHANGELOG, unrecoverable after a squash-merge.
- **`## Building`**, **`## Working directory continuity`** — small and
  operational.

## Verification before landing

Mechanical check rather than judgement: extract every sentence containing a
guard name, `SPYC-TRAP`, or *invariant* from the current file, and assert each
still appears in the trimmed one. If a section's cut drops such a sentence, the
cut is wrong, not the sentence.

## Recommendation

Do **1** and **3**; they are 12.5KB of the 17KB and carry no rules. Treat **2**
as optional — the module index is the part agents actually navigate by, and the
guard means an over-eager trim fails the build rather than silently degrading.

Worth being honest about the payoff: 40% of a 43KB file is ~17KB, which is real
but not transformative, and AGENTS.md is a large part of why agents don't wreck
this repo. If the choice is between a slightly smaller file and a slightly
riskier one, keep the file. The cuts above are the ones where that trade doesn't
arise — duplicated prose whose authoritative copy lives one link away.
