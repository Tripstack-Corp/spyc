# history-arc-08-recoverability-and-deps — Thread
Status: OPEN
Ball: Claude Code (caleb)
Topic: history-arc-08-recoverability-and-deps
Created: 2026-05-08T07:45:00.493522+00:00

---
Entry: Claude Code (caleb) 2026-05-08T07:45:00.493522+00:00
Role: scribe
Type: Note
Title: Framing: arc 08 — failure-mode hardening across three subgroups, cadence option A

Spec: scribe

tags: #history #arc-08

Arc title: `recoverability-and-deps`. Date span: 2026-05-03 (PR #13) to 2026-05-06 (PR #31); five PRs, two calendar clusters. Member PRs:

- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — "graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)" (commit f25d635, 2026-05-02 22:29 -04 / merged 2026-05-03 02:41 UTC).
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — "fix: route :undo / :graveyard to App's handler (v1.41.1)" (commit 24c49a0, 2026-05-02 22:48 -04 / merged 2026-05-03 03:06 UTC).
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06) — "fix: cap directory listings at 50k entries to avoid hangs (v1.41.15)" (commit f98604c, 2026-05-06 13:16 -04).
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06) — "fix: catch vt100 parser panics so spyc survives bad escape sequences (v1.41.17)" (commit bbdc415, 2026-05-06 13:48 -04).
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — "chore: upgrade vt100 0.15 → 0.16, ratatui 0.29 → 0.30 (v1.41.18)" (commit fc1789d, 2026-05-06 15:00 -04).

**Diagnosis: failure-mode-hardening shape, three independent failure modes, cadence option A.** Arc 08 reads as a closing-the-arc-shape — five PRs that share an audit-of-spyc-staying-alive register but do not share a single subsystem. The pattern catalogue at arc 06's framing offered four candidate fits; only one survives inspection of arc 08's diffs:

- **Pattern 5 (postmortem-twin) at the framing register** — rejected. The brief listed this as plausible because arc 08 closes the baseline phase, but no per-PR head's diff is structurally a postmortem. Each PR is a fix-or-feature, not a retrospective. The closing-the-arc-shape is calendrical, not voice-driven.
- **Arc 06 capability-and-correction (2+2)** — rejected on grain. Arc 06's two phase-α PRs widened the chord-prefix tree and arc 06's two phase-β PRs corrected dispatch as that surface grew. Arc 08's PR #14 corrects PR #13's routing, and PR #31 corrects PR #30's defensive framing, but the corrections are local pairs — they do not register as a phase-β over a phase-α.
- **Arcs 04 / 05 capability-accretion at finer grain** — rejected because the five PRs do not extend a single surface. Arc 05's eight PRs all touched the pager; arc 04's five PRs all touched git-awareness. Arc 08's PRs touch the file-system, the directory-listing path, and the pane parser respectively — three independent code surfaces.

The diagnosis the diffs actually support: **three failure-mode subgroups, each one defending a different way the prior spyc could lose state or die**:

- **Subgroup A — file-undo (PRs #13, #14)**: pre-arc, `R` was hard-delete; PR #13 ships graveyard + `:undo` + viewer; PR #14 routes the `:undo` command 25 minutes later when state's punt list misses it.
- **Subgroup B — directory-listing (PR #28)**: pre-arc, a 1M-entry tmp directory hung the event loop until the user killed the terminal; PR #28 caps `Listing::read` at 50k and flashes a hint when truncated.
- **Subgroup C — pane parser (PRs #30, #31)**: pre-arc, vt100 0.15's `screen.rs:934.unwrap()` on a particular nvim-exit-from-alt-screen byte stream took down the entire spyc process; PR #30 wraps `parser.process` in `catch_unwind` and respawns the parser on panic; PR #31 ships vt100 0.15 → 0.16 (plus ratatui 0.29 → 0.30, ansi-to-tui 7 → 8) as the proper upstream fix, keeping PR #30's safety net.

**Cadence choice: option A — five per-PR entries plus framing and closure (seven head entries).** Arc 03's option-A precedent (five PRs) inherits cleanly; phase-not-PR (option B) does not fit because the three subgroups don't decompose into investigation-then-harvest the way arc 02 did. Each subgroup's per-PR entry picks its own internal shape per the brief's per-entry-variety expectation:

- **PR #13 (graveyard)**: substantial. The system has multiple primitives — R-undo, per-entry tar.zst archive, `gy` viewer with p/P/dd/Z/Esc keymap, `:undo` one-shot, the 500 MB cascade-to-system-trash policy at startup. The entry will narrate the four primitives with code pointers.
- **PR #14 (routing fix)**: compact. Two lines added to `AppState::dispatch_command`'s punt list; the entry is the size of the diff.
- **PR #28 (directory cap)**: feature-shaped with a specific cap and rationale. The 50k constant, the `truncated: bool` field, the `read_capped` extraction for testability, three unit tests.
- **PR #30 (panic recovery)**: substantial. The recovery mechanism itself is structurally interesting — `catch_unwind` wrapping `parser.process`, parser-replacement at the same dimensions, the `panic = "abort"` → `panic = "unwind"` profile flip without which `catch_unwind` is a no-op in release.
- **PR #31 (dep upgrade)**: captures both halves of the dep bump (vt100 0.16 forces unicode-width ≥0.2.1 which forces the ratatui major + ansi-to-tui major) and answers the deferred §3 question against PR #31's diff.

**Two within-arc supersession pairs, with diagnostically-different acknowledgement shapes.** Arc 03 named the within-arc-twin shape on PR #26 → PR #29 (3.5 hours apart, silent supersession of a single branch). Arc 08 has two such pairs, both tighter in time:

- **PR #13 → PR #14 (25 minutes apart, 2026-05-03)**: feature-plus-immediate-hotfix. PR #14's CHANGELOG describes the routing bug accurately ("State's command dispatcher routes a fixed list of names to App's terminal-touching arms; `undo` and `graveyard` weren't on it") but does not cite PR #13 by number or title. The supersession is implicit; the bug-description-without-attribution shape mirrors arc 03's PR #26 → PR #29 pair, tighter in time. Same observation territory as the arc-03 within-arc twin.
- **PR #30 → PR #31 (49 minutes apart, 2026-05-06)**: defensive-then-corrected, with **explicit reframing** in PR #31's commit body. PR #30's commit message says "vt100 0.15 is unmaintained and has known unwrap() edge cases — this specific one fires while parsing the exit-from-alt-screen byte stream" and frames the upgrade as "the right-but-bigger fix" tracked in BUGS.md MAYBE. PR #31's commit body says verbatim "The vt100 bump is the proper fix for the `screen.rs:934.unwrap()` panic (caught defensively in v1.41.17). **Smaller than I'd previously framed it**" (commit fc1789d, 2026-05-06; emphasis preserved verbatim from the source). PR #30's BUGS.md MAYBE addition (added in the same diff) further self-corrects the unmaintained-0.15 framing: "We're pinned at 0.15.2; upstream is at 0.16.2 (active, not unmaintained — earlier notes saying 'the unmaintained 0.15' were inaccurate, corrected here)." PR #31 then deletes that MAYBE entry along with the mode-2026 and OSC 8 MAYBE entries it points at.

The two pairs land within four days, both intra-arc, both with the supersession happening within the hour. Their acknowledgement shapes differ — silent (PR #14) vs. explicitly self-correcting (PR #31). Naming this factually is arc 08's job; what the recurrence-of-tight-twins means across arcs 03 and 08 is for the insight layer.

**The PR #30 → PR #31 safety-net-bought-budget reframing.** Arc 02's investigation entry (= 01KR0YXXZRQR24CSNAK4Q7808T) projected: "the diff shape across the 49-minute gap points toward the panic-recovery as the safety net that buys budget for the major-version dep bump." The arc-02 author handed verification to arc 08. The diff verdict: **partly true, partly refuted**. PR #30 was framed by its own commit body as the only feasible 49-minute response because the upgrade was thought too big; PR #31's commit body explicitly reframes — the upgrade turned out to be six call-site adjustments in `pane/mod.rs` (`Parser::set_size` and `set_scrollback` moved to `Screen`, accessed via `parser.screen_mut()`) and one `&` borrow drop in `widget.rs` (because vt100 0.16's `Cell::contents` returns `&str` directly). The catch_unwind safety net **persists** under PR #31 ("costs nothing on the happy path, and any third-party parser can hit edge cases on some obscure escape sequence"), so PR #30's contribution is not retired — it is reframed from "until-the-upgrade" to "belt-and-suspenders." The safety-net-bought-budget reading is therefore: ⌐load-bearing-but-retained — PR #30's 38-line `process_bytes_safe` is not preconditional to PR #31's 12-line call-site update, but its `unwind` profile flip is preserved indefinitely.

**Suspect §3 (mode-2026) verification: option (a), answered.** Arc 02's investigation entry catalogued suspect §3 verbatim and deferred verification to arc 08. Arc 03's PR #29 entry restated the deferral. Arcs 04, 05, 06, 07 did not touch the question. Arc 08 owns the answer: PR #31's diff explicitly resolves it. PR #31's BUGS.md diff removes the MAYBE mode-2026 entry verbatim (the entry PR #12 had lifted from arc 02's gap-analysis suspect §3) and adds a `(fixed, v1.41.18)` block whose closing line reads verbatim: "Also retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks) — both should now parse correctly." PR #31's commit body restates the same resolution. The arc-02 → arc-08 cross-thread closes as **resolution**, not deferral. The PR #31 entry below carries the verbatim citation.

**The arc 02 → arc 08 cross-thread closure.** Three top suspects from arc 02's gap-analysis are now traceable to disposition:
- §1 (cursor-block, narrow → general) — answered in arc 03 by PR #29's three-condition guard.
- §2 (mouse, anywhere) — non-executed across the whole 22-day window; aligns with `onboarding-product-charter` non-goal at `ROADMAP.md:426-447`.
- §3 (synchronized-output mode 2026) — answered in arc 08 by PR #31's vt100 0.15 → 0.16 upgrade, per the maintainer's explicit claim in CHANGELOG / BUGS.md / commit body.

That is three for three on suspect-resolution-or-deferral across the arc network. Arc 08's PR #31 entry records this as factual handoff; whether the three-for-three-reading reads as a recurring-pattern (insight-layer turf) is not arc 08's to claim.

**PR #31 advisory-ignore reduction: zero.** The `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues five long-lived `cargo-deny` advisory ignores in `deny.toml:72-94`. PR #31's diff against `deny.toml` is empty — `git diff 105db8d^1..105db8d^2 -- deny.toml` returns no changes. All five ignores survive the upgrade unchanged: RUSTSEC-2026-0009 (time via syntect→plist; MSRV-blocked), RUSTSEC-2024-0320 (yaml-rust via syntect; build-time only), RUSTSEC-2025-0141 (bincode via syntect; complete-as-shipped), RUSTSEC-2024-0436 (paste via ratatui; build-time proc-macro), RUSTSEC-2017-0008 (serial via portable-pty; no alternative). The ratatui 0.29 → 0.30 bump in particular did not eliminate the `paste` ignore even though `paste` was specifically transit-via-ratatui per the catalogue's reason. Captured here factually; the per-PR PR #31 entry will not relitigate.

**Cross-thread back-link.** This thread continues from `history-overview` and the prior arc threads:

- `history-overview` framing (entry 0) = 01KR0TRFWT9W6WMFHC49YSW0BG.
- `history-overview` segmentation (entry 1) = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc 08's three-paragraph rationale at "Arc 08 — `recoverability-and-deps`"; PR #14 boundary-call called out).
- `history-overview` PR #5 special-handling (entry 2) = 01KR0TYF5F11DA8P5HNPA20DBK (arc 02 hub; PR #31's mode-2026 cross-reference).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry (entry 1) = 01KR0YXXZRQR24CSNAK4Q7808T (gap-analysis "Top suspects" §3 verbatim; deferral to arc 08).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry (entry 2) = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md MAYBE mode-2026 lift; "defer until someone notices").
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W (within-arc-twin-shape precedent on PR #26 → PR #29).
- `history-arc-06-input-and-overlays` story-tail = 01KR2GYQPQRX08SV980SPHHZ80 (capability-and-correction precedent ruled out as a fit for arc 08).
- `history-arc-07-codex-and-mcp-bridge` story-tail = 01KR2JM67RTQHQYN0223GTKH1V (single-tail-when-material-warrants precedent).
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ (current-state of pane PTY ownership; the surface PR #30/#31 touches).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (advisory-ignore catalogue and panic-unwind dependency note for `process_bytes_safe`).

The per-PR entries that follow this framing narrate PRs #13, #14, #28, #30, #31 in arc order (which is also wall-clock order). The closure entry names what the eight arcs together make legible without claiming what the insight layer will say. Tails follow the new tail voice contract.

Provenance:
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — `git show --stat 6b2be36`: 16 files, +1483/-54.
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — `git show --stat c7419c1`: 6 files, +14/-2.
- 306b43f (PR #28 fix/huge-directory-cap, 2026-05-06) — `git show --stat 306b43f`: 6 files, +102/-5.
- e39f462 (PR #30 fix/vt100-panic-recovery, 2026-05-06) — `git show --stat e39f462`: 5 files, +96/-10.
- 105db8d (PR #31 chore/vt100-and-ratatui-upgrade, 2026-05-06) — `git show --stat 105db8d`: 6 files, +783/-123 (the bulk in `Cargo.lock`).
- `git log` PR #31 commit body (commit fc1789d, 2026-05-06): "Smaller than I'd previously framed it" verbatim; "The catch_unwind safety net from v1.41.17 stays" verbatim.
- `git diff 105db8d^1..105db8d^2 -- BUGS.md`: removes the MAYBE mode-2026 entry verbatim; adds `(fixed, v1.41.18)` FIXED block with the "retires the two MAYBE entries about mode 2026 (synchronized output) and OSC 8 (terminal hyperlinks)" line.
- `git diff 105db8d^1..105db8d^2 -- deny.toml`: empty.
- `history-overview` segmentation entry = 01KR0TWHTC1MPK4KJ08Y9SPE6P (arc-08 rationale).
- `history-arc-02-lazygit-investigation-and-harvest` investigation entry = 01KR0YXXZRQR24CSNAK4Q7808T (suspect §3 verbatim and deferral).
- `history-arc-02-lazygit-investigation-and-harvest` harvest entry = 01KR0Z11CKNJRYEZ3T38EAFSC4 (BUGS.md MAYBE mode-2026 lift).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (deny.toml advisory-ignore catalogue; `process_bytes_safe` panic-unwind note).

<!-- Entry-ID: 01KR38QZ1XQ6EP2A4QC94DRD80 -->

---
Entry: Claude Code (caleb) 2026-05-08T07:46:54.483982+00:00
Role: scribe
Type: Note
Title: PR #13 (feat/graveyard-undo): four primitives at one diff — soft-delete cache, viewer, :undo, system-trash cascade — and the R-prompt blast-radius surfacing

Spec: scribe

tags: #history #arc-08

PR #13 is the first move in arc 08 and the file-undo subsystem move. The diff is the largest in arc 08 by every measure (16 files, +1483/-54; `src/state/graveyard.rs` is a 536-line new file; `src/app/mod.rs` gains 319 lines; three new runtime deps land — `tar`, `zstd`, `trash` — plus a `tempfile` promotion from dev-deps to runtime). The commit subject reads "graveyard: R-undo + per-entry tar.zst + system trash cascade (v1.41.0)" (commit f25d635, 2026-05-02 22:29 -04 / merged 2026-05-03 02:41 UTC). The minor cut to v1.41.0 — the only minor cut in arc 08 — is consistent with the diff shape: this is the new capability the rest of arc 08 reacts to.

The entry below narrates four primitives the diff lands at once, then names the one user-facing-prompt change the same diff slips in alongside, then catches the explicit non-goal in the commit body.

**Primitive 1 — the per-entry tar.zst archive schema.**

Before PR #13, items expelled from inventory used a paired `<uuid>.json` (metadata) + `<uuid>.dat` (raw payload bytes) graveyard schema in `$XDG_STATE_HOME/spyc/graveyard/`. PR #13 replaces this with a `<uuid>.json` + `<uuid>.tar.zst` pair — one schema for both single regular files and entire directory trees, so there is one read/write code path for both. The commit body names the design intent verbatim: "single regular files and entire directory trees use the same schema, so there's one read/write code path for both." Permission preservation is opt-in via `tar`'s `HeaderMode::Complete` (mode bits, mtime, best-effort UID/GID); restore opts in to `set_preserve_permissions(true)` and `set_preserve_mtime(true)`; `set_overwrite(false)` refuses to clobber existing files (commit body, fc25d635).

Pre-v1.41.0 paired `<uuid>.json` + `<uuid>.dat` entries are silently ignored by the new reader. The commit body's rationale, verbatim: "the graveyard is a transient soft-delete cache; major version bumps may lose recovery state — leaving the bytes in place is safer than trying to migrate." This is the diff shape that explains the choice not to ship a one-off migration tool — recovery state is not durable across major versions by design. `src/state/inventory.rs` (+55 / -55 net 0; same line count, full body rewrite) routes `move_to_graveyard` through the new schema so the graveyard is uniform from PR #13 forward.

**Primitive 2 — the `gy` viewer with its own keymap.**

A new viewer overlay surfaces the graveyard newest-first. The CHANGELOG names the keymap verbatim:

- **`gy`** — open the graveyard view (newest first); also toggles closed.
- Inside the view: **`p`** restore-to-cwd, **`P`** restore-to-original, **`dd`**/**`x`** purge entry to system trash, **`Z`** purge ALL (single-key confirm), **`Esc`**/**`gy`** close.

The asymmetry between `p` (lowercase, cwd) and `P` (uppercase, original) is the case-as-intent dispatch shape that arc 06's PR #10 (quickselect) ships in a different domain (lowercase yank, uppercase open). Arc 06 records the case-as-intent pattern factually; observing that PR #13's viewer ships the same shape independently is observational, not pattern-attribution turf.

Two new keymap actions land in `src/keymap/action.rs` (+4) and `src/keymap/resolver.rs` (+1). The viewer body itself is part of `src/app/mod.rs`'s 319-line gain.

**Primitive 3 — the `:undo` command as one-shot escape hatch.**

Distinct from the viewer: `:undo` restores the most-recent entry to its original path without opening the viewer. The commit body frames it as the "one-shot escape hatch." This is the command PR #14 will fix the routing for 25 minutes later; PR #13's diff lands the App-side handler but does not add `undo` to `AppState::dispatch_command`'s punt list. The drift is named in PR #14's entry, not here. From PR #13's vantage, `:undo` is a working command in the App's handler — the dispatch routing the punt list governs is the delta PR #14 catches.

**Primitive 4 — the 500 MB system-trash cascade at startup.**

A two-stage policy: the graveyard is the spyc-internal soft-delete cache (compressed, undo-able from the viewer); when it exceeds 500 MB, the **oldest entries cascade to the system trash** (FIFO) until the graveyard is back under cap. The commit body explains the design intent: "the unpacked content lands in Finder/Files with its original name so OS-native recovery still works." The cascade fires only at startup; a flash reports the count moved. Net pipeline: `R` → graveyard (compressed, spyc-recoverable) → system trash (uncompressed, OS-recoverable) when the cap is hit.

The 500 MB cap and FIFO-by-oldest are policy choices the diff doesn't justify against alternatives (size cap vs. age cap, 500 MB vs. some other number). The commit body names the cap and the policy without arguing for them; the catalogue of justifications — if any — lives outside this diff. Captured factually.

**The R-prompt blast-radius surfacing — a separate change in the same diff.**

Arc 08's framing flagged that PR #13 is substantial because the system has multiple primitives. There is also a behavior change to the existing `R` prompt that lands quietly inside the same PR. Pre-PR-#13: `R` prompted "remove N file(s)?". Post-PR-#13: `R` pre-walks any selected directory to count files inside and prompts "remove DIR (recursive, N file(s)) + M file(s)? (y/N)" — explicitly surfacing the recursive blast radius so that a reflexive `y` doesn't hide a 47-file delete behind a one-line prompt. The CHANGELOG bucket for this change is `### Changed`, distinct from the graveyard's `### Added` bucket. The change reads as a related-but-separate hardening: the graveyard makes hard-deletes recoverable, *and* the prompt makes the magnitude of recursive deletes legible, *and* both ship together because both address "you may not realize what `R` is about to do" from different angles. Naming the two changes as related-but-distinct is the read the diff supports; whether they read as one feature or two is for the reader.

**The explicit non-goal: xattrs / ACLs / macOS resource forks.**

The commit body names the scope boundary verbatim: "xattrs / ACLs / macOS resource forks are not preserved (out of scope for v1)." This is a recoverability gap the diff explicitly opts into — a graveyard entry restored from `.tar.zst` will not carry extended attributes back to the file. For spyc's audience (developers on macOS/Linux, file commander use case), the gap is a limitation worth knowing; for graveyard entries originating from sources that depend on extended attributes (Finder color labels, macOS quarantine bits, ACL-controlled corporate files), the `R` → restore round-trip silently strips state. The "(out of scope for v1)" framing positions this as a deferred item, not an indefinite one. No PR in the 22-day window appears to address it; flagging factually for the insight layer's negative-space catalogue.

**The dependency additions.**

New runtime deps in `Cargo.toml`: `tar = "0.4"`, `zstd = "0.13"`, `trash = "5"`. `tempfile` is promoted from dev-deps to runtime deps with a doc-comment justification verbatim from the diff: "tempfile is a runtime dep (not just dev-deps) because the graveyard's cascade-to-trash and legacy-migration paths stage files into a TempDir before handing them to the system trash." The commit body confirms: "All four pass cargo-deny." `Cargo.lock` carries 338 lines of transitive-dep changes from the four additions.

The `cargo-deny` check passing here is consistent with the `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) noting that new ignores must follow the documented-reason pattern. PR #13 adds none, which the entry flags as the positive-control case for the convention.

**Drift findings flagged for the insight layer.**

- The graveyard's cascade-to-system-trash policy fires only at startup. The diff does not justify why startup-only over event-driven (e.g. after each `R`) — startup-only is the chosen point but the alternatives are not catalogued. Captured for the insight layer's design-decision negative-space reading.
- The CHANGELOG's `### Added` bucket for the graveyard contains the `:undo` reference; the same `:undo` is broken at merge time because `AppState::dispatch_command`'s punt list does not include it. The drift between "documented capability" and "wired capability" is the seed `onboarding-risk-register` entry 0 names: "Bitten on `:undo` (v1.41.1) and `:limit` historically." PR #14 (next entry) is the historical instance the seed cites. PR #13's CHANGELOG ships the documented-capability promise; the wiring drift surfaces 25 minutes later. Captured for arc 08's two-pair recurrence reading and the eventual insight layer.
- Three new runtime deps land in one PR. Arc 01's PR #3 (security-hygiene) had cataloged the cargo-deny advisory-ignore baseline; this PR adds none, which is good, but the dep-graph expansion (338 lines of `Cargo.lock`) is the kind of change that increases the surface for future advisory ignores. Captured factually, no claim about future risk.
- The graveyard schema (`<uuid>.json` + `<uuid>.tar.zst`) is incompatible with pre-v1.41.0 paired entries by design. Users with prior `<uuid>.dat`-shaped entries find them silently ignored. The commit body names this as policy ("transient soft-delete cache"), but the user-visible artifact is "items deleted before v1.41.0 are no longer recoverable from spyc's viewer." Whether any user noticed this is not narratable from any commit in the 22-day window.
- The R-prompt blast-radius change ships under `### Changed` while the graveyard ships under `### Added`. The same diff makes two CHANGELOG buckets land at once. Arc 03's PR #26 (`Modifier::DIM` on the unfocused side) had the same shape — `### Changed` for a re-rendering of existing surface, distinct from feature add. Captured as observation; the "one-PR-two-CHANGELOG-buckets-with-internal-coherence" shape is recurring across arcs 03 and 08.

Provenance:
- 6b2be36 (PR #13 feat/graveyard-undo, 2026-05-03) — full PR.
- f25d635 — PR #13's feature-branch commit; commit body quoted verbatim throughout this entry.
- e210e58 → f25d635 — parent and tip SHAs for the diff inspection.
- `git diff e210e58..f25d635 -- src/state/graveyard.rs`: 536 lines new (file did not exist pre-PR-#13).
- `git diff e210e58..f25d635 -- src/app/mod.rs`: +319 net.
- `git diff e210e58..f25d635 -- src/app/state.rs`: +156 net.
- `git diff e210e58..f25d635 -- src/state/inventory.rs`: +55 / -55 net 0; rewrite-via-replacement of `move_to_graveyard`.
- `git diff e210e58..f25d635 -- Cargo.toml`: +10/-1 net; new deps `tar = "0.4"`, `zstd = "0.13"`, `trash = "5"`; `tempfile = "3"` promoted from `[dev-dependencies]` to `[dependencies]` with verbatim doc-comment.
- `git diff e210e58..f25d635 -- CHANGELOG.md`: 42 lines added; `### Added` graveyard block + `### Changed` R-prompt + inventory-schema blocks.
- `git diff e210e58..f25d635 -- BUGS.md`: 2 lines (the SMALL "no undo for R" entry presumably gets the v1.41.0 close treatment; verified by reading the diff, the line is updated to the FIXED bucket).
- `git diff e210e58..f25d635 -- src/keymap/action.rs`: +4 (two new viewer-action enum variants per CHANGELOG keymap).
- `git diff e210e58..f25d635 -- src/keymap/resolver.rs`: +1.
- `git diff e210e58..f25d635 -- README.md`: +17 (the user-visible recovery-surface description).
- `git diff e210e58..f25d635 -- FEATURES.md`: +34/-2 (the recovery-feature section).
- `git diff e210e58..f25d635 -- Cargo.lock`: +338/+0 net (transitive-dep additions from `tar`, `zstd`, `trash`, `tempfile` promotion).
- Commit body verbatim quotations: "single regular files and entire directory trees use the same schema, so there's one read/write code path for both"; "xattrs / ACLs / macOS resource forks are not preserved (out of scope for v1)"; "the graveyard is a transient soft-delete cache; major version bumps may lose recovery state — leaving the bytes in place is safer than trying to migrate"; "All four pass cargo-deny"; "13 new unit tests (6 in graveyard, others in dispatch / view)."
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA (cargo-deny convention; the punt-list foot-gun naming `:undo` v1.41.1 verbatim).

<!-- Entry-ID: 01KR38VEGHFT9JGRDCXXBFX8V1 -->

---
Entry: Claude Code (caleb) 2026-05-08T07:48:08.637710+00:00
Role: scribe
Type: Note
Title: PR #14 (fix/undo-command): two lines on the punt list, 25 minutes after PR #13, no PR-#13 citation

Spec: scribe

tags: #history #arc-08

PR #14 is the second move in arc 08 and the supersession-of-PR-#13's-routing move. The diff is the smallest in arc 08 (6 files, +14/-2; the load-bearing change is two lines added to one function in `src/app/state.rs`). The commit subject reads "fix: route :undo / :graveyard to App's handler (v1.41.1)" (commit 24c49a0, 2026-05-02 22:48 -04 / merged 2026-05-03 03:06 UTC) — 25 minutes after PR #13's merge.

**The two lines.**

`src/app/state.rs:1239` post-fix:

```rust
            || input == "undo"
            || input == "graveyard"
```

These join an existing chain inside `AppState::dispatch_command` that returns `CommandResult::NotHandled` for command names whose handler lives on App's terminal-touching half (per the `onboarding-architecture` seed entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ description of the partially-complete Elm refactor: "AppState::apply(action) returns an ApplyResult enum (Handled, OpenPager, Post(PostAction)) with no terminal access"). The chain pre-fix already included `pause`, `pause <args>`, `resume`, `resume <args>` (and an extended list above the snippet); without `undo` or `graveyard` on it, both fell through to the line-1247 "unknown command:" fallthrough.

**The bug PR #13 left.**

PR #13's diff added App's handler for `:undo` and `:graveyard`. The handler exists; the routing does not. The user-visible artifact, per PR #14's commit body verbatim: "Repro: type `:undo` → flash 'unknown command: undo'." The same bug applied to `:graveyard` (the command form of `gy` viewer-toggle). Both commands documented in PR #13's CHANGELOG `### Added` block fail at merge time of PR #13.

The 25-minute supersession means the bug existed in `main` for 25 minutes between PR #13's merge (2026-05-03 02:41 UTC) and PR #14's merge (2026-05-03 03:06 UTC). The arc-08-framing's two-pairs reading names this pair the tighter of the two within-arc twins; PR #30 → PR #31 is the looser at 49 minutes. Both pairs are intra-day; PR #13/#14 is intra-hour.

**The acknowledgement shape: bug-described, predecessor-not-cited.**

PR #14's CHANGELOG entry verbatim:

> "**`:undo` and `:graveyard` returned 'unknown command'.** State's command dispatcher routes a fixed list of names to App's terminal-touching arms; `undo` and `graveyard` weren't on it, so they hit the unknown-command fallthrough before App's handler could see them. Added both to the punt list."

The CHANGELOG names the bug accurately. It does not name PR #13. It does not say "regression introduced in v1.41.0" (the prior release). It does not link to PR #13's commit. The reader who reads only PR #14's CHANGELOG knows the routing-vs-handler split caused the bug; the reader does not know which prior PR shipped the broken pairing without grepping for `undo` against the PR-#13 commit body.

This shape is the diagnostic-of-interest. Arc 03's story-tail (= 01KR11S8RG29J98QKN1H0VAA6W) named the same shape on PR #29 verbatim: "What makes the supersession diagnostic isn't the guard-broadening per se — it's that nothing in either commit says 'this supersedes PR #5.'" Arc 08 has the same shape at a tighter time grain. PR #14's commit-body-and-CHANGELOG describe the bug exactly without acknowledging that 25 minutes earlier PR #13 had shipped the bug. The relationship between PR #13 and PR #14 lives in the diff's adjacency on `main`, not in any commit message text.

**The seed already carries the canonical reference.**

The `onboarding-risk-register` seed entry 0 (= 01KR0P9JC8Z3DF6FQ1GJPF3VKA) catalogues this exact bug as the named instance of the dual-`:command`-dispatch foot-gun verbatim: "Bitten on `:undo` (v1.41.1) and `:limit` historically." The v1.41.1 reference is PR #14's release tag. The seed predates arc 08 (entry timestamp 2026-05-07T07:44:05). So the foot-gun-with-named-instance documentation already exists at risk-register level by the time arc 08 reaches PR #14; the per-PR entry's job is to record that the seed's name-of-the-instance is grounded in this PR's two-line fix, not to relitigate the foot-gun.

**Drift findings flagged for the insight layer.**

- The supersession is silent at PR #14's commit-message level and explicit at the seed level. The cross-thread coverage is asymmetric: the seed (`onboarding-risk-register`) catalogues the bug class with this PR's release tag; PR #14's own commit message does not cite PR #13. Whether the insight layer reads this as a recurring "later observers reconstruct the supersession the original commits did not name" pattern is the insight layer's question. Captured factually here.
- PR #14 also adds two lines to `.gitignore` (per `git show --stat c7419c1`: 2 lines). The diff's content for `.gitignore` is not the load-bearing part of the PR but is bundled in. Whether the .gitignore additions are graveyard-related (e.g. ignoring `*.tar.zst` debug artifacts) or unrelated cleanup is verifiable from the diff but tangential to the routing fix. Captured factually; not load-bearing for the supersession reading.
- Patch-bump cadence: v1.41.0 → v1.41.1. The arc-01 reflection tail (= 01KR0XR504ZR10Y242JERT4K9S, restated at later arc-01 tails) named the v1.41.x cadence as the rhythm of patch-after-feature. PR #14 is one of the two within-arc-08 patch-bumps that follow a same-arc capability bump (the other is PR #28 → PR #30 → PR #31, where each is a patch on the prior). The patch-after-immediate-feature shape is intra-arc-08 here; arc 06's framing observed it across-arc (six minor cuts between PR #8's v1.39.0 and PR #32's v1.41.19). The pattern is recurrent; arc 08 records the instance.

Provenance:
- c7419c1 (PR #14 fix/undo-command, 2026-05-03) — full PR.
- 24c49a0 — PR #14's feature-branch commit; commit body quoted: "Repro: type `:undo` → flash 'unknown command: undo'."
- 6b2be36 → 24c49a0 — parent and tip SHAs for the diff inspection.
- `git diff 6b2be36..24c49a0 -- src/app/state.rs`: 2 lines added at line 1239 (`|| input == "undo"`, `|| input == "graveyard"`).
- `git diff 6b2be36..24c49a0 -- CHANGELOG.md`: 7 lines added under `[Unreleased]` `### Fixed`; the four-line description quoted verbatim above.
- `git diff 6b2be36..24c49a0 -- Cargo.toml`: `version = "1.41.0"` → `version = "1.41.1"`.
- `git diff 6b2be36..24c49a0 -- .gitignore`: 2 lines added (content not narrated in this entry; tangential).
- `git diff 6b2be36..24c49a0 -- CLAUDE.md`: 1 line added (content not narrated; tangential).
- `git diff 6b2be36..24c49a0 -- Cargo.lock`: 2 lines (the version bump's lockfile reflection).
- `onboarding-risk-register` entry 0 = 01KR0P9JC8Z3DF6FQ1GJPF3VKA — names "Bitten on `:undo` (v1.41.1)" verbatim as the foot-gun's historical instance.
- `onboarding-architecture` entry 0 = 01KR0P4W3ED1QZ8F44PFB2WPDZ — describes the partially-complete Elm refactor: "AppState::apply(action) returns an ApplyResult enum...with no terminal access. State transitions are pure-ish and unit-testable without a TUI." The dual-dispatch the foot-gun lives in.
- `history-arc-03-pane-behavior` story-tail = 01KR11S8RG29J98QKN1H0VAA6W — within-arc-twin precedent ("nothing in either commit says 'this supersedes PR #5'") quoted at this entry's acknowledgement-shape paragraph.
- `history-arc-08-recoverability-and-deps` framing entry = 01KR38QZ1XQ6EP2A4QC94DRD80.
- `history-arc-08-recoverability-and-deps` PR #13 entry = 01KR38VEGHFT9JGRDCXXBFX8V1.

<!-- Entry-ID: 01KR38XPJ07ZFQHH1TG6X461WN -->
